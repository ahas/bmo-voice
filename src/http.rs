//! The routes the voice answers (`docs/boundary-api.md`), their failures, and the router they stand on.
//! Everything a route takes and answers is [`crate::wire`]'s.

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as Sent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::{self, StreamExt};
use serde::de::DeserializeOwned;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::context::{Context, Wait};
use crate::events::{
  BRIEFING_CHANGED, BRIEFING_CREATED, BRIEFING_ENDED, BRIEFINGS_STOPPED, Events, SYNTHESIS_CREATED,
  SYNTHESIS_ENDED, TRANSCRIPTION_ENDED, Writing,
};
use crate::policy::POLICY;
use crate::script::reading::Language;
use crate::script::{spoken, unconverted};
use crate::speech::{card, node, synth};
use crate::store::{self, StoreError, voices};
use crate::units::arbiter::Ask;
use crate::wire::{
  Briefing, BriefingLine, BriefingQuery, BriefingStop, BriefingsStopped, Encoding, Event, EventQuery,
  Health, NewBriefing, NewEncoding, NewScriptReading, NewSynthesis, NewTranscription, Page, PageQuery,
  ScriptReading, Synthesis, SynthesisState, Transcription, Voice, VoiceServer, WaitOn, WireError,
};

/// How often an idle event stream says it is still there.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// The page size of a list where none is asked.
const PAGE: usize = 100;
/// The most a list's page holds.
const MOST_PAGE: usize = 1_000;

/// What every route stands on: the voice's context, its event log, and the token callers present.
#[derive(Clone)]
pub struct Served {
  pub context: Arc<Context>,
  pub events: Events,
  pub token: Option<Arc<String>>,
}

/// A refusal: its status, its code and its English sentence (`docs/boundary-api.md`, 「Codes」).
#[derive(Debug)]
pub struct Refused {
  pub status: StatusCode,
  pub code: &'static str,
  pub error: String,
}

impl Refused {
  pub fn new(status: StatusCode, code: &'static str, error: impl Into<String>) -> Self {
    Self {
      status,
      code,
      error: error.into(),
    }
  }

  /// 400 `bad-request`: the body, the query or the path is not the shape the route takes.
  pub fn bad_request(error: impl Into<String>) -> Self {
    Self::new(StatusCode::BAD_REQUEST, "bad-request", error)
  }

  /// 404 `not-found`: nothing stands at the key the request names.
  pub fn not_found(error: impl Into<String>) -> Self {
    Self::new(StatusCode::NOT_FOUND, "not-found", error)
  }

  /// 500 `failed`: a failure the route did not expect.
  pub fn failed(error: impl std::fmt::Display) -> Self {
    Self::new(StatusCode::INTERNAL_SERVER_ERROR, "failed", error.to_string())
  }
}

impl From<StoreError> for Refused {
  fn from(why: StoreError) -> Self {
    Self::failed(why)
  }
}

impl IntoResponse for Refused {
  fn into_response(self) -> Response {
    let body = WireError {
      code: self.code.to_owned(),
      error: self.error,
    };

    (self.status, Json(body)).into_response()
  }
}

// Every route, the health among them, wants the settings' token as its bearer; an empty token lets nobody in.
async fn authorised(State(served): State<Served>, request: Request, next: Next) -> Response {
  let presented = request
    .headers()
    .get(header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "));

  match (&served.token, presented) {
    (Some(expected), Some(token)) if token == expected.as_str() => next.run(request).await,
    _ => Refused::new(StatusCode::UNAUTHORIZED, "unauthorized", "a missing or wrong token").into_response(),
  }
}

async fn no_route(request: Request) -> Response {
  Refused::new(
    StatusCode::NOT_FOUND,
    "no-route",
    format!("no route answers {} {}", request.method(), request.uri().path()),
  )
  .into_response()
}

/// The service's routes.
pub fn router(served: Served) -> Router {
  Router::new()
    .route("/health", get(health))
    .route("/events", get(events))
    .route("/voices", get(list_voices))
    .route("/voices/{voiceName}", get(read_voice))
    .route("/syntheses", post(create_synthesis))
    .route("/syntheses/{synthesisId}", get(read_synthesis))
    .route("/syntheses/{synthesisId}/sound", get(synthesis_sound))
    .route("/briefings", post(create_briefing).get(list_briefings))
    .route("/briefings/stop", post(stop_briefings))
    .route("/briefings/{briefingId}", get(read_briefing))
    .route("/briefings/{briefingId}/line", post(name_line))
    .route("/briefings/{briefingId}/sound", get(briefing_sound))
    .route("/transcriptions", post(create_transcription))
    .route("/transcriptions/{transcriptionId}", get(read_transcription))
    .route("/encodings", post(create_encoding))
    .route("/script-readings", post(create_script_reading))
    .route("/voice-servers", get(list_voice_servers))
    .fallback(no_route)
    .layer(middleware::from_fn_with_state(served.clone(), authorised))
    .with_state(served)
}

/// `GET /health`.
async fn health() -> Json<Health> {
  Json(Health {
    ready: true,
    reason: None,
    version: crate::VERSION.to_owned(),
  })
}

fn body_of<T: DeserializeOwned>(read: Result<Json<T>, JsonRejection>) -> Result<T, Refused> {
  read
    .map(|Json(body)| body)
    .map_err(|why| Refused::bad_request(format!("the body is not the shape this route takes: {why}")))
}

fn query_of<T: DeserializeOwned>(read: Result<Query<T>, QueryRejection>) -> Result<T, Refused> {
  read
    .map(|Query(query)| query)
    .map_err(|why| Refused::bad_request(format!("the query is not the shape this route takes: {why}")))
}

fn page_size(limit: Option<u32>) -> Result<usize, Refused> {
  match limit {
    None => Ok(PAGE),
    Some(limit) if (1..=MOST_PAGE as u32).contains(&limit) => Ok(limit as usize),
    Some(limit) => Err(Refused::bad_request(format!("limit {limit} is not 1 to {MOST_PAGE}"))),
  }
}

// The kinds a reader narrowed the stream to.
fn narrowing(query: &EventQuery) -> Vec<String> {
  query
    .kinds
    .iter()
    .flat_map(|kinds| kinds.split(','))
    .map(str::trim)
    .filter(|kind| !kind.is_empty())
    .map(str::to_owned)
    .collect()
}

fn wanted(kinds: &[String], event: &Event) -> bool {
  kinds.is_empty() || kinds.contains(&event.kind)
}

/// `GET /events`: the stream, or a page of it to a reader asking for JSON.
/// Where a reader of the events starts: at the cursor it named, else where none is needed — a page at the
/// oldest event kept, a stream at the newest, so the stream carries the live events only. `first` and `last`
/// are the oldest and newest event numbers kept, none where none is kept.
pub fn event_start(named: Option<i64>, page: bool, first: Option<i64>, last: Option<i64>) -> i64 {
  match named {
    Some(after) => after,
    None if page => first.map_or(0, |first| first - 1),
    None => last.unwrap_or(0),
  }
}

async fn events(
  State(served): State<Served>,
  read: Result<Query<EventQuery>, QueryRejection>,
  request: Request,
) -> Result<Response, Refused> {
  let query = query_of(read)?;
  let resumed = request
    .headers()
    .get("last-event-id")
    .and_then(|value| value.to_str().ok())
    .and_then(|id| id.trim().parse::<i64>().ok());
  let named = query.after.or(resumed);
  let kinds = narrowing(&query);
  let wants_json = request
    .headers()
    .get(header::ACCEPT)
    .and_then(|value| value.to_str().ok())
    .is_some_and(|accept| accept.contains("application/json"));
  let first = served.events.first().await?;
  let last = served.events.last().await?;
  let after = event_start(named, wants_json, first, last);

  if let (Some(after), Some(first)) = (named, first)
    && after < first - 1
  {
    return Err(Refused::new(
      StatusCode::GONE,
      "gone",
      format!("the events after {after} are no longer kept; read the collections again"),
    ));
  }

  if wants_json {
    let limit = page_size(query.limit)?;
    let mut items = served.events.since(after, Some(&kinds), Some(limit)).await?;
    let more = items.len() > limit;

    items.truncate(limit);

    return Ok(Json(Page { items, more }).into_response());
  }

  // The live channel is heard before the window is read, so nothing published between the two is lost.
  let live = served.events.listen();
  let kept = served.events.since(after, Some(&kinds), None).await?;
  let replayed = kept.last().map_or(after, |event| event.id);
  let kinds = Arc::new(kinds);
  let events = served.events.clone();
  let live = stream::unfold(live, move |mut live| {
    let kinds = Arc::clone(&kinds);
    let events = events.clone();

    async move {
      loop {
        let heard = tokio::select! {
          heard = live.recv() => heard,
          () = events.stopped() => return None,
        };

        match heard {
          Ok(event) if event.id > replayed && wanted(kinds.as_ref(), &event) => {
            return Some((event, live));
          }
          Ok(_) => continue,
          Err(broadcast::error::RecvError::Lagged(_)) => continue,
          Err(broadcast::error::RecvError::Closed) => return None,
        }
      }
    }
  });
  let sent = stream::iter(kept).chain(live).map(|event| {
    let data = serde_json::to_string(&event).unwrap_or_default();

    Ok::<_, Infallible>(Sent::default().id(event.id.to_string()).event(event.kind).data(data))
  });
  let mut response = Sse::new(sent).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)).into_response();

  response
    .headers_mut()
    .insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-cache"));

  Ok(response)
}

fn voice_of(row: &voices::VoiceRow) -> Voice {
  let engine = row.engine().unwrap_or_default();

  Voice {
    name: row.name.clone(),
    language: row.language.clone(),
    checkpoint: row.checkpoint.clone(),
    served: engine == POLICY.engine,
    engine,
    emotions: POLICY.emotions.clone(),
    changed_at: row.set_at.saturating_mul(1_000),
  }
}

/// `GET /voices`.
async fn list_voices(
  State(served): State<Served>,
  read: Result<Query<PageQuery>, QueryRejection>,
) -> Result<Json<Page<Voice>>, Refused> {
  let query = query_of(read)?;
  let limit = page_size(query.limit)?;
  let rows = voices::rows(&served.context.db).await?;
  let mut items: Vec<Voice> = rows
    .iter()
    .filter(|row| query.after.as_ref().is_none_or(|after| row.name > *after))
    .map(voice_of)
    .collect();
  let more = items.len() > limit;

  items.truncate(limit);

  Ok(Json(Page { items, more }))
}

/// `GET /voices/{voiceName}`.
async fn read_voice(
  State(served): State<Served>,
  Path(voice_name): Path<String>,
) -> Result<Json<Voice>, Refused> {
  let row = voices::voice(&served.context.db, &voice_name)
    .await?
    .ok_or_else(|| Refused::not_found(format!("there is no voice {voice_name}")))?;

  Ok(Json(voice_of(&row)))
}

/// `POST /syntheses`.
async fn create_synthesis(
  State(served): State<Served>,
  read: Result<Json<NewSynthesis>, JsonRejection>,
) -> Result<Response, Refused> {
  let asked = body_of(read)?;

  if asked.text.trim().is_empty() {
    return Err(Refused::bad_request("a synthesis speaks a text; this one is empty"));
  }

  within_bounds(&asked.text, asked.spoken.as_deref())?;

  let key = checked_key(asked.key.as_deref())?;

  if let Some(key) = key
    && let Some(held) = store::syntheses::keyed(&served.context.db, key).await?
  {
    return Ok((StatusCode::OK, Json(read_by_caller(&served.context, &held))).into_response());
  }

  let row = voices::voice(&served.context.db, &asked.voice_name)
    .await?
    .ok_or_else(|| Refused::not_found(format!("there is no voice {}", asked.voice_name)))?;

  served_voice(&row)?;
  known_emotion(asked.emotion.as_deref())?;

  let language = line_language(asked.language.as_deref(), &asked.text, &row)?;
  let spoken = checked_spoken(&asked.text, asked.spoken.as_deref(), language)?;
  let at = crate::now_ms();
  // Held from the row's insert to its event, so no change told of it is numbered before its creation.
  let writing = served.events.write().await;
  let taken = store::syntheses::Asked {
    voice_name: &asked.voice_name,
    text: &asked.text,
    spoken: spoken.as_deref(),
    language: language.code(),
    emotion: asked.emotion.as_deref(),
    key,
    asked_by: asked.asked_by.as_deref(),
    at,
  };
  let id = match store::syntheses::insert(&served.context.db, &taken).await? {
    store::syntheses::Taken::New(id) => id,
    // Another call with this key took its synthesis in in the moment between: that one is the answer.
    store::syntheses::Taken::Held(held) => {
      drop(writing);

      return Ok((StatusCode::OK, Json(read_by_caller(&served.context, &held))).into_response());
    }
  };
  let synthesis = Synthesis {
    id,
    voice_name: asked.voice_name.clone(),
    text: asked.text.clone(),
    spoken: spoken.clone(),
    emotion: asked.emotion.clone(),
    key: key.map(str::to_owned),
    asked_by: asked.asked_by.clone(),
    state: SynthesisState::Waiting,
    wait: None,
    waiting_on: None,
    failure: None,
    duration_ms: None,
    created_at: at,
  };

  writing.record(SYNTHESIS_CREATED, &synthesis).await?;
  drop(writing);

  let mut voice = row;
  if let Some(style) = asked.style {
    voice.style = style;
  }
  if let Some(seed) = asked.seed {
    voice.seed = seed;
  }

  let words = Words {
    text: asked.text,
    spoken,
    language,
    asked_by: asked.asked_by,
  };

  served.context.reads.read(id, served.context.clocks.now());
  eprintln!("voice: synthesis {id} taken in, in {} ({})", voice.name, words.language.code());
  speak_apart(Arc::clone(&served.context), served.events.clone(), id, None, voice, words);

  Ok((StatusCode::CREATED, Json(waited(&served.context, synthesis))).into_response())
}

// The longest key a caller may name a line by.
const MOST_KEY: usize = 200;

// A caller's key for its line, held to its shape before anything is looked up or taken in: not blank, and at most
// `MOST_KEY` characters (docs/boundary-api.md, 「Syntheses」).
fn checked_key(key: Option<&str>) -> Result<Option<&str>, Refused> {
  match key {
    Some(key) if key.trim().is_empty() || key.chars().count() > MOST_KEY => {
      Err(Refused::bad_request(format!("a key is not blank and is at most {MOST_KEY} characters")))
    }
    other => Ok(other),
  }
}

// The most characters of written words a line is taken in with.
const MOST_TEXT: usize = 10_000;

// A line's written words and the spoken words a caller gave, held to the bounds every line taken in is spoken within
// whatever its length (docs/boundary-api.md, 「Syntheses」).
fn within_bounds(text: &str, spoken: Option<&str>) -> Result<(), Refused> {
  if text.chars().count() > MOST_TEXT {
    return Err(Refused::bad_request(format!("a text is at most {MOST_TEXT} characters")));
  }

  let most_spoken = crate::speech::spoken::MOST_SPOKEN;

  if spoken.is_some_and(|words| words.chars().count() > most_spoken) {
    return Err(Refused::bad_request(format!("spoken words are at most {most_spoken} characters")));
  }

  Ok(())
}

// The briefing whose synthesis holds a key, answered 200; a key a synthesis that speaks no briefing holds is a
// conflict. A briefing's row is written right after its synthesis's, so one another call is taking in this moment is
// looked for a little while before the key is said to be a plain synthesis's.
async fn held_briefing(served: &Served, held: &store::syntheses::Row) -> Result<Response, Refused> {
  for _ in 0..20 {
    if let Some(id) = store::briefings::of_synthesis(&served.context.db, held.id).await? {
      return Ok((StatusCode::OK, Json(briefing_of(&served.context.db, id).await?)).into_response());
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
  }

  Err(Refused::new(
    StatusCode::CONFLICT,
    "conflict",
    format!("the key is held by synthesis {}, which speaks no briefing", held.id),
  ))
}

// The language a line is said in (docs/boundary-api.md, 「The spoken words」): the one it names, else the one its
// written words are in, else its voice's; a voice of a language the spoken words have no check for says only lines
// that name one or whose words say one.
fn line_language(named: Option<&str>, written: &str, row: &voices::VoiceRow) -> Result<Language, Refused> {
  if let Some(named) = named {
    return Language::parse(named)
      .ok_or_else(|| Refused::bad_request(format!("there is no language {named}")));
  }

  if let Some(said) = Language::of_line(written) {
    return Ok(said);
  }

  Language::parse(&row.language).ok_or_else(|| {
    Refused::failed(format!(
      "{} speaks {}, a language the spoken words have no check for",
      row.name, row.language
    ))
  })
}

// The spoken words a caller gave, held to the check against the written words before anything is taken in
// (docs/boundary-api.md, 「The spoken words」): never mended, refused naming the first word that failed.
fn checked_spoken(
  written: &str,
  spoken: Option<&str>,
  language: Language,
) -> Result<Option<String>, Refused> {
  let Some(spoken) = spoken else {
    return Ok(None);
  };

  match unconverted::found(written, spoken, language) {
    Some(found) => {
      Err(Refused::new(StatusCode::UNPROCESSABLE_ENTITY, "unconverted-run", found.to_string()))
    }
    None => Ok(Some(spoken.trim().to_owned())),
  }
}

// A line's two texts, the written words and the spoken words where they already stand, the language the voice
// says them in, and who its caller said asks it, the asker its spoken words are derived for.
struct Words {
  text: String,
  spoken: Option<String>,
  language: Language,
  asked_by: Option<String>,
}

// A voice of an engine the voice does not speak is kept and never spoken, so it is refused before anything is taken
// in, naming the engine its folder says and the one served.
fn served_voice(row: &voices::VoiceRow) -> Result<(), Refused> {
  let engine = row.engine().map_err(Refused::failed)?;

  if engine == POLICY.engine {
    return Ok(());
  }

  Err(Refused::new(
    StatusCode::UNPROCESSABLE_ENTITY,
    "retired-voice",
    format!(
      "{} is a {engine} voice, an engine the voice no longer speaks; it is kept and never spoken, and the voices of {} are",
      row.name, POLICY.engine
    ),
  ))
}

// An emotion outside the policy's is refused before anything is taken in.
fn known_emotion(emotion: Option<&str>) -> Result<(), Refused> {
  match emotion {
    Some(emotion) if !POLICY.emotions.iter().any(|known| known == emotion) => Err(Refused::new(
      StatusCode::UNPROCESSABLE_ENTITY,
      "unknown-emotion",
      format!("the emotions are {}", POLICY.emotions.join(", ")),
    )),
    _ => Ok(()),
  }
}

// The spoken words a synthesis taken in without them is derived (docs/boundary-api.md, 「How the engine derives the
// spoken words」), kept as its own once they stand; what the derivation waits on is told as the synthesis's wait, and
// once they stand it waits for its card. A derivation whose tries are spent answers why the last one failed.
async fn derived_words(
  context: &Context,
  id: i64,
  language: Language,
  text: &str,
  asker: Option<&str>,
) -> Result<String, String> {
  let mut told = |why: &str| {
    eprintln!("voice: synthesis {id} waits on its spoken words: {why}");
    context.waits.set(id, Wait::words(why));
  };
  let words = crate::speech::spoken::derived(context, id, language, text, asker, &mut told).await?;

  if let Err(why) = store::syntheses::set_spoken(&context.db, id, &words).await {
    eprintln!("voice: synthesis {id}'s spoken words were not kept: {why}");
  }

  context.waits.set(id, Wait::asking());

  Ok(words)
}

// A line's speaking run apart from the answer, a task of the service's one runtime, which every waiting line shares;
// its stop signal stands before the task does, and a briefing's end is kept and told as its synthesis ends. A plain
// synthesis is held by its caller's reads; a briefing's is the room's and has no caller's clock.
fn speak_apart(
  context: Arc<Context>,
  events: Events,
  synthesis: i64,
  briefing: Option<i64>,
  voice: voices::VoiceRow,
  words: Words,
) {
  let stop = context.stops.watch(synthesis);

  // Named before the speaking first tells it, so no read finds a synthesis that has not ended waiting on nothing.
  context.waits.set(
    synthesis,
    match words.spoken {
      Some(_) => Wait::asking(),
      None => Wait::words(crate::speech::spoken::DERIVING),
    },
  );

  match briefing {
    Some(briefing) => {
      tokio::spawn(brief(context, events, briefing, synthesis, voice, words, stop));
    }
    None => {
      tokio::spawn(speak(context, events, synthesis, voice, words, stop, Caller::Held));
    }
  }
}

// Whether a synthesis is held by its caller's reads, and so ends once its caller is gone.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Caller {
  Held,
  Room,
}

// How a synthesis's speaking came to its end: its own, its stop, or its caller gone.
enum Ended {
  Own,
  Stopped,
  CallerGone,
}

// One synthesis spoken unless its stop signal or its caller's clock comes first: either drops its speaking where it
// stands, the derivation, the wait for its card or the call on its node, ends that call on the node, and gives back
// whatever card its line holds. The stop's own route has written and told its end; a caller gone fails the synthesis
// here, saying so. Answers whether it ran to its own end.
async fn speak(
  context: Arc<Context>,
  events: Events,
  id: i64,
  voice: voices::VoiceRow,
  words: Words,
  stop: CancellationToken,
  caller: Caller,
) -> bool {
  let derives = words.spoken.is_none();
  let ended = tokio::select! {
    () = speak_line(&context, events.clone(), id, voice, words) => Ended::Own,
    () = stop.cancelled() => Ended::Stopped,
    () = caller_gone(&context, id), if caller == Caller::Held => Ended::CallerGone,
  };

  context.stops.forget(id);
  context.reads.forget(id);

  let caller_gone = match ended {
    Ended::Own => return true,
    Ended::Stopped => false,
    Ended::CallerGone => true,
  };

  let stood = context.waits.stood(id);

  context.waits.clear(id);
  card::give_back_work(&context.units.arbiter, &context.settings.demands_folder(), &node::work_key(id))
    .await;

  // A derivation ended under its try leaves its slot and session to its owner's release.
  if derives && let Some(agency) = &context.units.agency {
    crate::speech::spoken::give_back_owned(agency, id).await;
  }

  if caller_gone {
    let why =
      format!("its caller has gone, not having read it for {}", node::span(context.clocks.caller_gone));

    failed(&context, events, id, stood.as_deref(), &why).await;
  } else {
    eprintln!("voice: synthesis {id} stopped{}", standing(stood.as_deref()));
  }

  false
}

// Ends once the synthesis's caller has not read it for the caller's clock, counted from its last read.
async fn caller_gone(context: &Context, id: i64) {
  let clocks = &context.clocks;

  loop {
    let now = clocks.now();
    let due = context.reads.last(id).unwrap_or(now) + clocks.caller_gone;

    if now >= due {
      return;
    }

    clocks.until(due).await;
  }
}

// Where a synthesis stood as it ended, as its journal line names it after its end's word; nothing where it waited on
// nothing or its reason names the node itself.
fn standing(stood: Option<&str>) -> String {
  stood.map(|stood| format!(" {stood}")).unwrap_or_default()
}

// A synthesis failed with its reason, written to the journal with where it stood and, where it had not ended
// meanwhile, told as its end under the same hold of the event log as its failure is written.
async fn failed(context: &Context, events: Events, id: i64, stood: Option<&str>, why: &str) {
  eprintln!("voice: synthesis {id} failed{}: {why}", standing(stood));

  let writing = events.write().await;

  match store::syntheses::fail(&context.db, id, why).await {
    Ok(true) => {
      let _ = ended_synthesis(&writing, &context.db, id).await;
    }
    Ok(false) => {}
    Err(why) => eprintln!("voice: synthesis {id}'s failure was not written: {why}"),
  }
}

// Where a line stands as its speaking tells it: waiting for its card, or on the node of the card granted.
enum Told {
  Card(String),
  Node {
    demand: i64,
    node: String,
    stage: String,
    said: String,
  },
}

impl Told {
  fn of(stage: node::Stage<'_>) -> Self {
    match stage {
      node::Stage::Waiting(why) => Self::Card(why.to_owned()),
      node::Stage::OnNode {
        demand,
        node,
        stage,
      } => Self::Node {
        demand,
        node: node.to_owned(),
        stage: stage.stage.clone(),
        said: stage.said.clone(),
      },
    }
  }

  fn state(&self) -> SynthesisState {
    match self {
      Self::Card(_) => SynthesisState::Waiting,
      Self::Node { .. } => SynthesisState::Speaking,
    }
  }

  fn wait(&self) -> Wait {
    match self {
      Self::Card(why) => Wait {
        on: WaitOn::Demand,
        node: None,
        demand: None,
        said: why.clone(),
      },
      Self::Node {
        demand, node, said, ..
      } => Wait {
        on: WaitOn::Node,
        node: Some(node.clone()),
        demand: Some(*demand),
        said: format!("{node}: {said}"),
      },
    }
  }

  // What the journal tells of it: a line for each change in kind, the card's reason, or the node and the stage of
  // the line there, and none for a piece of a long line, which is the same stage spoken on.
  fn kind(&self) -> String {
    match self {
      Self::Card(why) => format!("waits for its card: {why}"),
      Self::Node {
        demand,
        node,
        stage,
        ..
      } => format!("on {node} (demand {demand}): {stage}"),
    }
  }

  fn journal(&self, id: i64) {
    match self {
      Self::Card(why) => eprintln!("voice: synthesis {id} waits for its card: {why}"),
      Self::Node {
        demand, node, said, ..
      } => eprintln!("voice: synthesis {id} on {node} (demand {demand}): {said}"),
    }
  }
}

// One synthesis spoken on the card the arbiter's master grants it, its state, what it waits on and the event of its
// end told as they come, and each change in kind written to the journal. A synthesis that ended meanwhile, stopped,
// takes no state and tells no end of its own.
async fn speak_line(context: &Context, events: Events, id: i64, voice: voices::VoiceRow, words: Words) {
  let text = match words.spoken {
    Some(spoken) => spoken,
    None => {
      match derived_words(context, id, words.language, &words.text, words.asked_by.as_deref()).await {
        Ok(derived) => derived,
        Err(why) => {
          context.waits.clear(id);

          return failed(context, events, id, None, &why).await;
        }
      }
    }
  };
  let out = sound_path(context, id);
  if let Some(folder) = out.parent().map(std::path::Path::to_path_buf) {
    let _ = tokio::fs::create_dir_all(folder).await;
  }

  let ask = Ask {
    service_name: node::SERVICE.to_owned(),
    count: 1,
    work_key: Some(node::work_key(id)),
    reference: Some(format!("voice synthesis {id} in {}", voice.name)),
  };
  let speech = node::Speech {
    units: &context.units,
    settings: &context.settings,
    voice: &voice,
    text: &text,
    language: words.language,
    out: &out,
    ask: &ask,
    clocks: &context.clocks,
  };
  // Each stage is handed over as it changes and written here, since the speaking cannot wait on the store.
  let (said, mut heard) = tokio::sync::mpsc::unbounded_channel::<Told>();
  let mut told = move |stage: node::Stage<'_>| {
    let _ = said.send(Told::of(stage));
  };
  let speaking = node::speak_on_card(speech, &mut told);
  let mut state = SynthesisState::Waiting;
  let mut kind = String::new();

  tokio::pin!(speaking);

  let ended = loop {
    tokio::select! {
      ended = &mut speaking => break ended,
      Some(now) = heard.recv() => {
        context.waits.set(id, now.wait());

        if now.kind() != kind {
          now.journal(id);
          kind = now.kind();
        }

        if now.state() != state {
          let _ = store::syntheses::set_state(&context.db, id, now.state(), None).await;
          state = now.state();
        }
      }
    }
  };

  let stood = context.waits.stood(id);

  context.waits.clear(id);

  let (heard, duration_ms) = match ended {
    Ok(heard) => {
      let seconds = tokio::fs::read(&out)
        .await
        .ok()
        .and_then(|bytes| synth::ogg_opus_seconds(&bytes))
        .unwrap_or(heard.spoken.seconds);

      (heard, Some((seconds * 1_000.0).round().max(0.0) as i64))
    }
    Err(why) => {
      // A node's failure names its node in its reason; any other is told with where the line stood.
      let stood = stood.as_deref().filter(|_| !matches!(why, node::NotSpoken::OnNode(..)));

      return failed(context, events, id, stood, &why.to_string()).await;
    }
  };

  let writing = events.write().await;

  match store::syntheses::set_state(&context.db, id, SynthesisState::Done, duration_ms).await {
    Ok(true) => {
      eprintln!(
        "voice: synthesis {id} done on {}: {} ms of sound, {} ms waited for its card, the node loaded the voice in {:.1} s and spoke in {:.1} s ({})",
        heard.node,
        duration_ms.unwrap_or_default(),
        heard.waited_ms,
        heard.spoken.load_seconds,
        heard.spoken.speak_seconds,
        heard.spoken.load_parts
      );
      let _ = ended_synthesis(&writing, &context.db, id).await;
    }
    Ok(false) => {}
    Err(why) => eprintln!("voice: synthesis {id} was spoken but its state was not written: {why}"),
  }
}

// A synthesis's end told as `synthesis.ended`, within the hold its end was written under.
async fn ended_synthesis(
  writing: &Writing<'_>,
  db: &sea_orm::DatabaseConnection,
  id: i64,
) -> Result<(), Refused> {
  let Some(row) = store::syntheses::get(db, id).await? else {
    return Ok(());
  };

  writing.record(SYNTHESIS_ENDED, &row.synthesis()).await?;

  Ok(())
}

fn sound_path(context: &Context, id: i64) -> PathBuf {
  context.settings.voice_folder().join("syntheses").join(format!("{id}.ogg"))
}

/// `GET /syntheses/{synthesisId}`.
async fn read_synthesis(
  State(served): State<Served>,
  Path(id): Path<i64>,
) -> Result<Json<Synthesis>, Refused> {
  let row = store::syntheses::get(&served.context.db, id)
    .await?
    .ok_or_else(|| Refused::not_found(format!("there is no synthesis {id}")))?;

  Ok(Json(read_by_caller(&served.context, &row)))
}

// A read of a synthesis that has not ended starts its caller's clock again; an ended one has no clock left.
fn caller_read(context: &Context, row: &store::syntheses::Row) {
  if matches!(row.state, SynthesisState::Waiting | SynthesisState::Speaking) {
    context.reads.read(row.id, context.clocks.now());
  }
}

// A synthesis as its caller reads it: what it waits on where it has not ended, its state the one that wait makes it,
// since the store's is written a moment after the wait changes, and the read starting its caller's clock again
// (docs/boundary-api.md, 「What it waits on」, 「The clocks」).
fn read_by_caller(context: &Context, row: &store::syntheses::Row) -> Synthesis {
  caller_read(context, row);

  waited(context, row.synthesis())
}

// A synthesis with what it waits on where it has not ended, its state the one that wait makes it.
fn waited(context: &Context, mut synthesis: Synthesis) -> Synthesis {
  if let Some((wait, said)) = context.waits.of(synthesis.id)
    && matches!(synthesis.state, SynthesisState::Waiting | SynthesisState::Speaking)
  {
    synthesis.state = wait.on.state();
    synthesis.wait = Some(wait);
    synthesis.waiting_on = Some(said);
  }

  synthesis
}

/// `GET /syntheses/{synthesisId}/sound`.
async fn synthesis_sound(
  State(served): State<Served>,
  Path(id): Path<i64>,
  request: Request,
) -> Result<Response, Refused> {
  let row = store::syntheses::get(&served.context.db, id)
    .await?
    .ok_or_else(|| Refused::not_found(format!("there is no synthesis {id}")))?;

  caller_read(&served.context, &row);

  if row.state != SynthesisState::Done {
    return Err(Refused::not_found(format!("synthesis {id} has no sound yet")));
  }

  let kept = sound_path(&served.context, id);

  if !kept.is_file() {
    return Err(Refused::not_found(format!("synthesis {id} has no sound")));
  }

  let mime = "audio/ogg".parse().expect("a mime type");
  let answered = ServeFile::new_with_mime(&kept, &mime).oneshot(request).await.map_err(Refused::failed)?;

  Ok(answered.into_response())
}

/// `POST /briefings`: a line spoken for a room, answered at once, its synthesis speaking apart and its end told
/// as `briefing.ended`.
async fn create_briefing(
  State(served): State<Served>,
  read: Result<Json<NewBriefing>, JsonRejection>,
) -> Result<Response, Refused> {
  let asked = body_of(read)?;

  if asked.text.trim().is_empty() {
    return Err(Refused::bad_request("a briefing speaks a text; this one is empty"));
  }

  within_bounds(&asked.text, asked.spoken.as_deref())?;

  if asked.kind.names_its_line() && asked.message_id.is_none() {
    return Err(Refused::bad_request("a full reading names the line it reads"));
  }

  let key = checked_key(asked.key.as_deref())?;

  if let Some(key) = key
    && let Some(held) = store::syntheses::keyed(&served.context.db, key).await?
  {
    return held_briefing(&served, &held).await;
  }

  let voice = voices::voice(&served.context.db, &asked.voice_name)
    .await?
    .ok_or_else(|| Refused::not_found(format!("there is no voice {}", asked.voice_name)))?;

  served_voice(&voice)?;
  known_emotion(asked.emotion.as_deref())?;

  let db = &served.context.db;
  let script = spoken::finished_script(&spoken::spoken_script(&asked.text));
  let script = if script.trim().is_empty() { asked.text.trim().to_owned() } else { script };
  let language = line_language(asked.language.as_deref(), &script, &voice)?;
  let spoken_words = checked_spoken(&script, asked.spoken.as_deref(), language)?;
  let at = crate::now_ms();
  // Held from the rows' insert to their events, so no change told of them, a stop's above all, is numbered before
  // their creation.
  let writing = served.events.write().await;
  let taken = store::syntheses::Asked {
    voice_name: &asked.voice_name,
    text: &script,
    spoken: spoken_words.as_deref(),
    language: language.code(),
    emotion: asked.emotion.as_deref(),
    key,
    asked_by: asked.asked_by.as_deref(),
    at,
  };
  let synthesis_id = match store::syntheses::insert(db, &taken).await? {
    store::syntheses::Taken::New(id) => id,
    store::syntheses::Taken::Held(held) => {
      drop(writing);

      return held_briefing(&served, &held).await;
    }
  };
  let id = store::briefings::insert(
    db,
    &store::briefings::Taken {
      chat_id: asked.chat_id,
      message_id: asked.message_id,
      kind: asked.kind,
      title: asked.title.as_deref().map(str::trim).filter(|title| !title.is_empty()),
      synthesis_id,
      plays: asked.plays.unwrap_or(true),
      at,
    },
  )
  .await?;
  let briefing = briefing_of(db, id).await?;

  if let Some(synthesis) = store::syntheses::get(db, synthesis_id).await? {
    writing.record(SYNTHESIS_CREATED, &synthesis.synthesis()).await?;
  }
  writing.record(BRIEFING_CREATED, &briefing).await?;
  drop(writing);

  let words = Words {
    text: script,
    spoken: spoken_words,
    language,
    asked_by: asked.asked_by,
  };

  eprintln!(
    "voice: synthesis {synthesis_id} taken in for briefing {id}, in {} ({})",
    voice.name,
    words.language.code()
  );
  speak_apart(Arc::clone(&served.context), served.events.clone(), synthesis_id, Some(id), voice, words);

  Ok((StatusCode::CREATED, Json(briefing)).into_response())
}

/// Every line a stopped process left unspoken taken up (docs/boundary-api.md, 「Syntheses」): each synthesis still
/// `waiting` or `speaking` spoken again from its words in the language it was taken in with, a briefing's ended as
/// its synthesis ends, and one whose voice is gone or not served failed. Answers how many were taken up.
pub async fn take_up(context: Arc<Context>, events: Events) -> Result<usize, StoreError> {
  let db = &context.db;
  let mut taken = 0;

  for row in store::syntheses::unfinished(db).await? {
    let id = row.id;
    let voice = voices::voice(db, &row.voice_name).await?.filter(|voice| served_voice(voice).is_ok());
    let language = row.language.as_deref().and_then(Language::parse);
    let language = language.or_else(|| voice.as_ref().and_then(|voice| Language::parse(&voice.language)));
    let (Some(voice), Some(language)) = (voice, language) else {
      let why = format!("its voice {} is gone or not served", row.voice_name);

      failed(&context, events.clone(), id, None, &why).await;

      if let Ok(Some(briefing)) = store::briefings::of_synthesis(db, id).await {
        end_briefing(db, &events, briefing, None).await;
      }

      continue;
    };

    let _ = store::syntheses::set_state(db, id, SynthesisState::Waiting, None).await;

    context.reads.read(id, context.clocks.now());
    eprintln!(
      "voice: synthesis {id} taken up from an earlier process, in {} ({})",
      voice.name,
      language.code()
    );

    let briefing = store::briefings::of_synthesis(db, id).await?;
    let words = Words {
      text: row.text,
      spoken: row.spoken,
      language,
      asked_by: row.asked_by,
    };

    speak_apart(Arc::clone(&context), events.clone(), id, briefing, voice, words);
    taken += 1;
  }

  Ok(taken)
}

// One briefing spoken as its synthesis is, and its end kept and told; a briefing stopped meanwhile keeps the end its
// stop wrote and told.
async fn brief(
  context: Arc<Context>,
  events: Events,
  id: i64,
  synthesis_id: i64,
  voice: voices::VoiceRow,
  words: Words,
  stop: CancellationToken,
) {
  if !speak(Arc::clone(&context), events.clone(), synthesis_id, voice, words, stop, Caller::Room).await {
    return;
  }

  let db = &context.db;
  let spoken = store::syntheses::get(db, synthesis_id).await.ok().flatten();
  let sound = sound_path(&context, synthesis_id).to_string_lossy().into_owned();
  let done = spoken
    .as_ref()
    .filter(|row| row.state == SynthesisState::Done)
    .map(|row| (row.duration_ms.unwrap_or(0) as f64 / 1_000.0, sound.as_str()));

  end_briefing(db, &events, id, done).await;
}

// A briefing's end written, done where `done` gives its sound's length and place, and told as `briefing.ended` under
// the same hold of the event log; one that has ended already, by its stop, keeps that end and is not told again.
async fn end_briefing(
  db: &sea_orm::DatabaseConnection,
  events: &Events,
  id: i64,
  done: Option<(f64, &str)>,
) {
  let writing = events.write().await;

  match store::briefings::ended(db, id, crate::now_ms(), done).await {
    Ok(true) => {}
    Ok(false) => return,
    Err(why) => {
      eprintln!("voice: briefing {id}'s end was not written: {why}");
      return;
    }
  }

  match store::briefings::get(db, id).await {
    Ok(Some(row)) => {
      if let Err(why) = writing.record(BRIEFING_ENDED, &row.briefing).await {
        eprintln!("voice: briefing {id}'s end was not published: {why}");
      }
    }
    Ok(None) => {}
    Err(why) => eprintln!("voice: briefing {id} was not read back: {why}"),
  }
}

async fn briefing_of(db: &sea_orm::DatabaseConnection, id: i64) -> Result<Briefing, Refused> {
  store::briefings::get(db, id)
    .await?
    .map(|row| row.briefing)
    .ok_or_else(|| Refused::not_found(format!("there is no briefing {id}")))
}

/// `GET /briefings`.
async fn list_briefings(
  State(served): State<Served>,
  read: Result<Query<BriefingQuery>, QueryRejection>,
) -> Result<Json<Page<Briefing>>, Refused> {
  let query = query_of(read)?;
  let limit = page_size(query.limit)?;
  let mut items: Vec<Briefing> = store::briefings::page(&served.context.db, &query, limit + 1)
    .await?
    .into_iter()
    .map(|row| row.briefing)
    .collect();
  let more = items.len() > limit;

  items.truncate(limit);

  Ok(Json(Page { items, more }))
}

/// `GET /briefings/{briefingId}`.
async fn read_briefing(
  State(served): State<Served>,
  Path(id): Path<i64>,
) -> Result<Json<Briefing>, Refused> {
  Ok(Json(briefing_of(&served.context.db, id).await?))
}

/// `POST /briefings/{briefingId}/line`: the line a summary speaks named, or the summary let go without one.
async fn name_line(
  State(served): State<Served>,
  Path(id): Path<i64>,
  read: Result<Json<BriefingLine>, JsonRejection>,
) -> Result<Json<Briefing>, Refused> {
  let asked = body_of(read)?;
  let db = &served.context.db;
  let standing = briefing_of(db, id).await?;

  if standing.kind.names_its_line() {
    return Err(Refused::new(
      StatusCode::CONFLICT,
      "conflict",
      format!("briefing {id} is a full reading, whose line is never moved"),
    ));
  }

  let writing = served.events.write().await;

  store::briefings::set_line(db, id, asked.message_id).await?;

  let briefing = briefing_of(db, id).await?;

  writing.record(BRIEFING_CHANGED, &briefing).await?;

  Ok(Json(briefing))
}

/// `POST /briefings/stop`: every briefing of a room and a kind, or of every room and every kind, that has not ended
/// stopped at once, its speaking ended where it runs, each told as it ends, and then the stop itself told for the
/// listeners to fall silent.
async fn stop_briefings(
  State(served): State<Served>,
  read: Result<Json<BriefingStop>, JsonRejection>,
) -> Result<Json<Page<Briefing>>, Refused> {
  let asked = body_of(read)?;
  let db = &served.context.db;
  let writing = served.events.write().await;
  let stopped = store::briefings::stop(db, asked.chat_id, asked.kind, crate::now_ms()).await?;

  for one in &stopped {
    served.context.stops.stop(one.synthesis);
  }

  let mut items = Vec::with_capacity(stopped.len());

  for one in &stopped {
    if let Some(synthesis) = store::syntheses::get(db, one.synthesis).await? {
      writing.record(SYNTHESIS_ENDED, &synthesis.synthesis()).await?;
    }

    let briefing = briefing_of(db, one.briefing).await?;

    writing.record(BRIEFING_ENDED, &briefing).await?;
    items.push(briefing);
  }

  let told = BriefingsStopped {
    chat_id: asked.chat_id,
    kind: asked.kind,
    briefing_ids: stopped.iter().map(|one| one.briefing).collect(),
  };

  writing.record(BRIEFINGS_STOPPED, &told).await?;

  Ok(Json(Page { items, more: false }))
}

/// `GET /briefings/{briefingId}/sound`.
async fn briefing_sound(
  State(served): State<Served>,
  Path(id): Path<i64>,
  request: Request,
) -> Result<Response, Refused> {
  let briefing = briefing_of(&served.context.db, id).await?;
  let Some(synthesis_id) = briefing.synthesis_id.filter(|_| briefing.state == SynthesisState::Done) else {
    return Err(Refused::not_found(format!("briefing {id} has no sound the engine keeps")));
  };
  let kept = sound_path(&served.context, synthesis_id);

  if !kept.is_file() {
    return Err(Refused::not_found(format!("briefing {id} has no sound")));
  }

  let mime = "audio/ogg".parse().expect("a mime type");
  let answered = ServeFile::new_with_mime(&kept, &mime).oneshot(request).await.map_err(Refused::failed)?;

  Ok(answered.into_response())
}

/// `POST /transcriptions`.
async fn create_transcription(
  State(served): State<Served>,
  read: Result<Json<NewTranscription>, JsonRejection>,
) -> Result<Response, Refused> {
  let asked = body_of(read)?;

  if asked.files.is_empty() {
    return Err(Refused::bad_request("a transcription takes at least one file"));
  }

  let at = crate::now_ms();
  let id = store::transcriptions::insert(&served.context.db, &asked.language, at).await?;
  let transcription = Transcription {
    id,
    language: asked.language.clone(),
    state: "waiting".to_owned(),
    text: None,
    created_at: at,
  };
  let context = Arc::clone(&served.context);
  let events = served.events.clone();
  let language = asked.language;
  let files = asked.files;

  tokio::spawn(transcribe(context, events, id, language, files));

  Ok((StatusCode::CREATED, Json(transcription)).into_response())
}

// One transcription taken down, its state and the event of its end told as it comes.
async fn transcribe(context: Arc<Context>, events: Events, id: i64, language: String, files: Vec<String>) {
  let _ = store::transcriptions::set_state(&context.db, id, "running", None).await;
  let (state, text) = match synth::transcribe(&language, &files, &context.settings).await {
    Ok(text) => ("done", Some(text)),
    Err(why) => {
      eprintln!("voice: transcription {id} failed: {why}");
      ("failed", None)
    }
  };

  let writing = events.write().await;

  if let Err(why) = store::transcriptions::set_state(&context.db, id, state, text.as_deref()).await {
    eprintln!("voice: transcription {id} was not written: {why}");
  }

  let Ok(Some(row)) = store::transcriptions::get(&context.db, id).await else {
    return;
  };

  if let Err(why) = writing.record(TRANSCRIPTION_ENDED, &row.transcription()).await {
    eprintln!("voice: transcription {id}'s end was not published: {why}");
  }
}

/// `GET /transcriptions/{transcriptionId}`.
async fn read_transcription(
  State(served): State<Served>,
  Path(id): Path<i64>,
) -> Result<Json<Transcription>, Refused> {
  let row = store::transcriptions::get(&served.context.db, id)
    .await?
    .ok_or_else(|| Refused::not_found(format!("there is no transcription {id}")))?;

  Ok(Json(row.transcription()))
}

/// `POST /encodings`: a reference recording encoded into a sound here and now, answered once it stands.
async fn create_encoding(
  State(served): State<Served>,
  read: Result<Json<NewEncoding>, JsonRejection>,
) -> Result<Response, Refused> {
  let asked = body_of(read)?;
  let source = PathBuf::from(&asked.from_wav);

  if !source.is_file() {
    return Err(Refused::new(
      StatusCode::UNPROCESSABLE_ENTITY,
      "unreadable-source",
      format!("there is no file at {}", asked.from_wav),
    ));
  }

  let at = crate::now_ms();
  let id = store::encodings::insert(&served.context.db, &asked.from_wav, at).await?;
  let out = served.context.settings.voice_folder().join("encodings").join(format!("{id}.ogg"));
  let _ = tokio::fs::create_dir_all(out.parent().unwrap_or(&out)).await;

  match synth::encode(&source, &out, &served.context.settings).await {
    Ok(()) => {
      store::encodings::set_state(&served.context.db, id, "done", Some(&out.to_string_lossy())).await?;
      let encoding = Encoding {
        id,
        state: "done".to_owned(),
        created_at: at,
      };

      Ok((StatusCode::CREATED, Json(encoding)).into_response())
    }
    Err(why) => {
      let _ = store::encodings::set_state(&served.context.db, id, "failed", None).await;

      Err(Refused::failed(why))
    }
  }
}

/// `POST /script-readings`.
async fn create_script_reading(
  State(_served): State<Served>,
  read: Result<Json<NewScriptReading>, JsonRejection>,
) -> Result<Json<ScriptReading>, Refused> {
  let asked = body_of(read)?;

  if let Some(language) = &asked.language
    && Language::parse(language).is_none()
  {
    return Err(Refused::bad_request(format!("there is no language {language}")));
  }

  let script = spoken::spoken_script(&asked.text);
  let sentences = sentences_of(&script);

  Ok(Json(ScriptReading { script, sentences }))
}

// A script's sentences: the text cut after each end mark that closes a sentence, its own marks kept. A mark
// inside a figure or an address closes nothing.
fn sentences_of(script: &str) -> Vec<String> {
  let mut sentences = Vec::new();
  let mut current = String::new();
  let mut characters = script.chars().peekable();

  while let Some(character) = characters.next() {
    current.push(character);

    let end = matches!(character, '.' | '!' | '?' | '。' | '！' | '？' | '…')
      && characters.peek().is_none_or(|next| next.is_whitespace());

    if end {
      if !current.trim().is_empty() {
        sentences.push(current.trim().to_owned());
      }
      current.clear();
    }
  }

  if !current.trim().is_empty() {
    sentences.push(current.trim().to_owned());
  }

  sentences
}

/// `GET /voice-servers`.
async fn list_voice_servers(
  State(served): State<Served>,
  read: Result<Query<PageQuery>, QueryRejection>,
) -> Result<Json<Page<VoiceServer>>, Refused> {
  let query = query_of(read)?;
  let limit = page_size(query.limit)?;
  let names = voices::node_names(&served.context.db).await?;
  let rows = voices::rows(&served.context.db).await?;
  // The page's nodes are chosen before any is reached, so no borrow of the query is held across a call.
  let chosen: Vec<String> = names
    .into_iter()
    .filter(|name| query.after.as_ref().is_none_or(|after| name > after))
    .take(limit + 1)
    .collect();
  let mut items = Vec::new();

  for name in chosen {
    let (state, loaded) =
      match node::node_shell(&served.context.units, name.clone(), &served.context.clocks).await {
        Ok(shell) => match node::health(&shell).await {
          Ok(health) => ("running", health.loaded),
          Err(_) => ("absent", None),
        },
        Err(_) => ("absent", None),
      };
    let voice_names = match loaded {
      Some(loaded) => rows
        .iter()
        .filter(|row| row.checkpoint.rsplit('/').next().is_some_and(|last| last == loaded))
        .map(|row| row.name.clone())
        .collect(),
      None => Vec::new(),
    };

    items.push(VoiceServer {
      node_name: name,
      state: state.to_owned(),
      voice_names,
    });
  }

  let more = items.len() > limit;

  items.truncate(limit);

  Ok(Json(Page { items, more }))
}
