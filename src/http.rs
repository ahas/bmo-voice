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
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::context::Context;
use crate::events::{
  BRIEFING_CHANGED, BRIEFING_CREATED, BRIEFING_ENDED, Events, SYNTHESIS_CREATED, SYNTHESIS_ENDED,
  TRANSCRIPTION_ENDED,
};
use crate::policy::POLICY;
use crate::script::reading::Language;
use crate::script::{spoken, unconverted};
use crate::speech::{node, synth};
use crate::store::{self, StoreError, voices};
use crate::units::arbiter::Ask;
use crate::wire::{
  Briefing, BriefingKind, BriefingLine, BriefingQuery, Encoding, Event, EventQuery, Health, NewBriefing,
  NewEncoding, NewScriptReading, NewSynthesis, NewTranscription, Page, PageQuery, ScriptReading, Synthesis,
  SynthesisState, Transcription, Voice, VoiceServer, WireError,
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
    version: env!("CARGO_PKG_VERSION").to_owned(),
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
  let after = query.after.or(resumed).unwrap_or(0);
  let kinds = narrowing(&query);
  let wants_json = request
    .headers()
    .get(header::ACCEPT)
    .and_then(|value| value.to_str().ok())
    .is_some_and(|accept| accept.contains("application/json"));

  if let Some(first) = served.events.first().await?
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
  let live = stream::unfold(live, move |mut live| {
    let kinds = Arc::clone(&kinds);

    async move {
      loop {
        match live.recv().await {
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
  let row = voices::voice(&served.context.db, &asked.voice_name)
    .await?
    .ok_or_else(|| Refused::not_found(format!("there is no voice {}", asked.voice_name)))?;

  served_voice(&row)?;
  known_emotion(asked.emotion.as_deref())?;

  let language = line_language(asked.language.as_deref(), &row)?;
  let spoken = checked_spoken(asked.spoken.as_deref(), language)?;
  let at = crate::now_ms();
  let id = store::syntheses::insert(
    &served.context.db,
    &asked.voice_name,
    &asked.text,
    spoken.as_deref(),
    language.code(),
    asked.emotion.as_deref(),
    at,
  )
  .await?;
  let synthesis = Synthesis {
    id,
    voice_name: asked.voice_name.clone(),
    text: asked.text.clone(),
    spoken: spoken.clone(),
    emotion: asked.emotion.clone(),
    state: SynthesisState::Waiting,
    waiting_on: None,
    duration_ms: None,
    created_at: at,
  };

  served.events.write().await.record(SYNTHESIS_CREATED, &synthesis).await?;

  let mut voice = row;
  if let Some(style) = asked.style {
    voice.style = style;
  }
  if let Some(seed) = asked.seed {
    voice.seed = seed;
  }

  let context = Arc::clone(&served.context);
  let events = served.events.clone();
  let text = asked.text;
  // The speaking runs apart from the answer, on a runtime of its own: the call into the voice nodes holds
  // their shells and their waits, which no request's task is to carry.
  tokio::task::spawn_blocking(move || {
    if let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
      runtime.block_on(speak(
        context,
        events,
        id,
        voice,
        Words {
          text,
          spoken,
          language,
        },
      ));
    }
  });

  Ok((StatusCode::CREATED, Json(synthesis)).into_response())
}

// The language a line is said in: the one it names, else its voice's; a voice of a language the spoken words have
// no check for says only lines that name one.
fn line_language(named: Option<&str>, row: &voices::VoiceRow) -> Result<Language, Refused> {
  if let Some(named) = named {
    return Language::parse(named)
      .ok_or_else(|| Refused::bad_request(format!("there is no language {named}")));
  }

  Language::parse(&row.language).ok_or_else(|| {
    Refused::failed(format!(
      "{} speaks {}, a language the spoken words have no check for",
      row.name, row.language
    ))
  })
}

// The spoken words a caller gave, held to the check before anything is taken in (docs/boundary-api.md, 「The spoken
// words」): never mended, refused naming the first word that failed.
fn checked_spoken(spoken: Option<&str>, language: Language) -> Result<Option<String>, Refused> {
  let Some(spoken) = spoken else {
    return Ok(None);
  };

  match unconverted::unconverted(spoken, language) {
    Some(found) => {
      Err(Refused::new(StatusCode::UNPROCESSABLE_ENTITY, "unconverted-run", found.to_string()))
    }
    None => Ok(Some(spoken.trim().to_owned())),
  }
}

// A line's two texts, the written words and the spoken words where they already stand, and the language the voice
// says them in.
struct Words {
  text: String,
  spoken: Option<String>,
  language: Language,
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
// spoken words」), kept as its own once they stand; what the derivation waits on is told as the synthesis's wait.
async fn derived_words(context: &Context, id: i64, language: Language, text: &str) -> String {
  let mut told = |why: &str| context.waits.set(id, Some(why));
  let words = crate::speech::spoken::derived(context, id, language, text, &mut told).await;

  if let Err(why) = store::syntheses::set_spoken(&context.db, id, &words).await {
    eprintln!("voice: synthesis {id}'s spoken words were not kept: {why}");
  }

  context.waits.set(id, None);

  words
}

// One synthesis spoken on the card the arbiter's master grants it, its state, what it waits on and the event of its
// end told as they come.
async fn speak(context: Arc<Context>, events: Events, id: i64, voice: voices::VoiceRow, words: Words) {
  let text = match words.spoken {
    Some(spoken) => spoken,
    None => derived_words(&context, id, words.language, &words.text).await,
  };
  let out = sound_path(&context, id);
  if let Some(folder) = out.parent().map(std::path::Path::to_path_buf) {
    let _ = tokio::fs::create_dir_all(folder).await;
  }

  let ask = Ask {
    service_name: node::SERVICE.to_owned(),
    count: 1,
    work_key: Some(format!("voice:synthesis:{id}")),
    reference: Some(format!("voice synthesis {id} in {}", voice.name)),
  };
  let speech = node::Speech {
    units: &context.units,
    settings: &context.settings,
    voice: &voice,
    text: &text,
    out: &out,
    cap_ms: POLICY.run_cap_ms,
    ask: &ask,
  };
  // Each stage is handed over as it changes and written here, since the speaking cannot wait on the store.
  let (said, mut heard) = tokio::sync::mpsc::unbounded_channel::<Option<String>>();
  let mut told = move |stage: node::Stage<'_>| {
    let _ = said.send(match stage {
      node::Stage::Waiting(why) => Some(why.to_owned()),
      node::Stage::Speaking => None,
    });
  };
  let speaking = node::speak_on_card(speech, &mut told);
  let mut state = SynthesisState::Waiting;

  tokio::pin!(speaking);

  let ended = loop {
    tokio::select! {
      ended = &mut speaking => break ended,
      Some(waiting_on) = heard.recv() => {
        let now = if waiting_on.is_some() { SynthesisState::Waiting } else { SynthesisState::Speaking };

        context.waits.set(id, waiting_on.as_deref());

        if now != state {
          let _ = store::syntheses::set_state(&context.db, id, now, None).await;
          state = now;
        }
      }
    }
  };

  context.waits.set(id, None);

  let duration_ms = match ended {
    Ok(heard) => {
      let seconds = tokio::fs::read(&out)
        .await
        .ok()
        .and_then(|bytes| synth::ogg_opus_seconds(&bytes))
        .unwrap_or(heard.spoken.seconds);

      Some((seconds * 1_000.0).round().max(0.0) as i64)
    }
    Err(why) => {
      eprintln!("voice: synthesis {id} failed: {why}");

      let _ = store::syntheses::set_state(&context.db, id, SynthesisState::Failed, None).await;
      let _ = ended_synthesis(events, context.db.clone(), id).await;

      return;
    }
  };

  if let Err(why) = store::syntheses::set_state(&context.db, id, SynthesisState::Done, duration_ms).await {
    eprintln!("voice: synthesis {id} was spoken but its state was not written: {why}");
  }
  let _ = ended_synthesis(events, context.db.clone(), id).await;
}

async fn ended_synthesis(events: Events, db: sea_orm::DatabaseConnection, id: i64) -> Result<(), Refused> {
  let Some(row) = store::syntheses::get(&db, id).await? else {
    return Ok(());
  };

  events.write().await.record(SYNTHESIS_ENDED, &row.synthesis()).await?;

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
  let mut synthesis = row.synthesis();

  synthesis.waiting_on = served.context.waits.of(id);

  Ok(Json(synthesis))
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

  if asked.kind == BriefingKind::Full && asked.message_id.is_none() {
    return Err(Refused::bad_request("a full reading names the line it reads"));
  }

  let voice = voices::voice(&served.context.db, &asked.voice_name)
    .await?
    .ok_or_else(|| Refused::not_found(format!("there is no voice {}", asked.voice_name)))?;

  served_voice(&voice)?;
  known_emotion(asked.emotion.as_deref())?;

  let db = &served.context.db;
  let language = line_language(asked.language.as_deref(), &voice)?;
  let spoken_words = checked_spoken(asked.spoken.as_deref(), language)?;
  let script = spoken::finished_script(&spoken::spoken_script(&asked.text));
  let script = if script.trim().is_empty() { asked.text.trim().to_owned() } else { script };
  let at = crate::now_ms();
  let synthesis_id = store::syntheses::insert(
    db,
    &asked.voice_name,
    &script,
    spoken_words.as_deref(),
    language.code(),
    asked.emotion.as_deref(),
    at,
  )
  .await?;
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

  {
    let writing = served.events.write().await;

    if let Some(synthesis) = store::syntheses::get(db, synthesis_id).await? {
      writing.record(SYNTHESIS_CREATED, &synthesis.synthesis()).await?;
    }
    writing.record(BRIEFING_CREATED, &briefing).await?;
  }

  let context = Arc::clone(&served.context);
  let events = served.events.clone();

  tokio::task::spawn_blocking(move || {
    if let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
      let words = Words {
        text: script,
        spoken: spoken_words,
        language,
      };

      runtime.block_on(brief(context, events, id, synthesis_id, voice, words));
    }
  });

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
      let _ = store::syntheses::set_state(db, id, SynthesisState::Failed, None).await;
      let _ = ended_synthesis(events.clone(), db.clone(), id).await;

      if let Ok(Some(briefing)) = store::briefings::of_synthesis(db, id).await {
        let _ = store::briefings::ended(db, briefing, crate::now_ms(), None).await;
      }

      eprintln!(
        "voice: synthesis {id} left unspoken failed: its voice {} is gone or not served",
        row.voice_name
      );
      continue;
    };

    let _ = store::syntheses::set_state(db, id, SynthesisState::Waiting, None).await;

    let briefing = store::briefings::of_synthesis(db, id).await?;
    let words = Words {
      text: row.text,
      spoken: row.spoken,
      language,
    };
    let context = Arc::clone(&context);
    let events = events.clone();

    tokio::task::spawn_blocking(move || {
      if let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
        match briefing {
          Some(briefing) => runtime.block_on(brief(context, events, briefing, id, voice, words)),
          None => runtime.block_on(speak(context, events, id, voice, words)),
        }
      }
    });
    taken += 1;
  }

  Ok(taken)
}

// One briefing spoken as its synthesis is, and its end kept and told.
async fn brief(
  context: Arc<Context>,
  events: Events,
  id: i64,
  synthesis_id: i64,
  voice: voices::VoiceRow,
  words: Words,
) {
  speak(Arc::clone(&context), events.clone(), synthesis_id, voice, words).await;

  let db = &context.db;
  let spoken = store::syntheses::get(db, synthesis_id).await.ok().flatten();
  let sound = sound_path(&context, synthesis_id).to_string_lossy().into_owned();
  let done = spoken
    .as_ref()
    .filter(|row| row.state == SynthesisState::Done)
    .map(|row| (row.duration_ms.unwrap_or(0) as f64 / 1_000.0, sound.as_str()));

  if let Err(why) = store::briefings::ended(db, id, crate::now_ms(), done).await {
    eprintln!("voice: briefing {id}'s end was not written: {why}");
  }

  match store::briefings::get(db, id).await {
    Ok(Some(row)) => {
      if let Err(why) = events.write().await.record(BRIEFING_ENDED, &row.briefing).await {
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

  if standing.kind == BriefingKind::Full {
    return Err(Refused::new(
      StatusCode::CONFLICT,
      "conflict",
      format!("briefing {id} is a full reading, whose line is never moved"),
    ));
  }

  store::briefings::set_line(db, id, asked.message_id).await?;

  let briefing = briefing_of(db, id).await?;

  served.events.write().await.record(BRIEFING_CHANGED, &briefing).await?;

  Ok(Json(briefing))
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

  tokio::task::spawn_blocking(move || {
    if let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
      runtime.block_on(transcribe(context, events, id, language, files));
    }
  });

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

  if let Err(why) = store::transcriptions::set_state(&context.db, id, state, text.as_deref()).await {
    eprintln!("voice: transcription {id} was not written: {why}");
  }

  let Ok(Some(row)) = store::transcriptions::get(&context.db, id).await else {
    return;
  };

  if let Err(why) = events.write().await.record(TRANSCRIPTION_ENDED, &row.transcription()).await {
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
    let (state, loaded) = match node::node_shell(&served.context.units, name.clone()).await {
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
