//! The routes the service answers (`docs/boundary-api.md`, 「What the voice serves」).

use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, Stream, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::hub::Frame;
use super::{Refused, Served};
use crate::briefing::runner::{self, kept_sound};
use crate::briefing::{self, Launch};
use crate::policy::POLICY;
use crate::script::reading::{self, Language};
use crate::script::{figures, polish, spoken};
use crate::store::{briefings, voices};

// How often a keep frame goes to a listener that hears nothing else.
const KEEP_EVERY: Duration = Duration::from_secs(5);

/// The service is up.
pub async fn health() -> Json<Value> {
  Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}

/// A launch as a caller writes it; the receiver's and the artifact unit's names for the same fields both read.
#[derive(Debug, Deserialize)]
pub struct Launching {
  #[serde(alias = "chat_id")]
  chat: i64,
  text: String,
  #[serde(alias = "by", default)]
  started_by: Option<String>,
  kind: Option<String>,
  title: Option<String>,
  message: Option<i64>,
  #[serde(default)]
  awaits_line: Option<bool>,
  document: Option<i64>,
  emotion: Option<String>,
  agent: Option<String>,
  via: Option<String>,
}

// A refusal of the launch, with the status its code stands under.
fn refused(refusal: briefing::Refusal) -> Refused {
  let status = match refusal.code {
    "unknown_emotion" => StatusCode::UNPROCESSABLE_ENTITY,
    "switched_off" | "voiceless" => StatusCode::CONFLICT,
    "not_up" => StatusCode::SERVICE_UNAVAILABLE,
    _ => StatusCode::BAD_REQUEST,
  };

  Refused::new(status, refusal.code, refusal.error)
}

// A launch's body read into the launch it names: a title or the kind `summary` is a summary, the kind `notice` a
// notice turn's opening kept as a summary, the kind `reading` a full reading of the line it names, and a document or
// the kind `narration` a document's sound.
fn launch_of(asked: Launching) -> Result<Launch, Refused> {
  let bad = |why: &str| Refused::new(StatusCode::BAD_REQUEST, "bad_launch", why);
  let kind = asked.kind.as_deref().unwrap_or(if asked.document.is_some() {
    "narration"
  } else if asked.title.is_some() {
    "summary"
  } else {
    "briefing"
  });
  let started_by = asked.started_by.unwrap_or_else(|| match kind {
    "notice" => format!("notice:{}", asked.chat),
    "reading" => format!("console:{}", asked.chat),
    "narration" => asked.document.map_or_else(String::new, |document| format!("archive-doc:{document}")),
    _ => format!("chat:{}", asked.chat),
  });
  let mut launch = Launch {
    chat_id: asked.chat,
    text: asked.text,
    agent: asked.agent,
    started_by,
    emotion: asked.emotion,
    via: asked.via,
    ..Launch::default()
  };

  match kind {
    "briefing" => {}
    "summary" | "notice" => {
      launch.summary = true;
      launch.title = asked.title.filter(|title| !title.trim().is_empty());
      launch.voices = asked.message.filter(|_| asked.awaits_line != Some(true));
    }
    "reading" => {
      launch.keep = true;
      launch.voices =
        Some(asked.message.ok_or_else(|| bad("a full reading names the line it reads: message"))?);
    }
    "narration" => {
      launch.document =
        Some(asked.document.ok_or_else(|| bad("a narration names its document: document"))?);
    }
    other => {
      return Err(bad(&format!(
        "a briefing's kind is briefing, notice, summary, reading or narration; {other} is not"
      )));
    }
  }

  Ok(launch)
}

/// A briefing launched: its run and the unit that speaks it, or why nothing was started.
pub async fn launch(
  State(served): State<Served>,
  Json(asked): Json<Launching>,
) -> Result<Response, Refused> {
  let launch = launch_of(asked)?;

  match briefing::launch(&served.context, &launch).await {
    Ok(Ok(launched)) => Ok(
      (
        StatusCode::CREATED,
        Json(json!({ "run": launched.run_id.to_string(), "unit": launched.unit })),
      )
        .into_response(),
    ),
    Ok(Err(refusal)) => Err(refused(refusal)),
    Err(why) => Err(Refused::failed(why)),
  }
}

#[derive(Debug, Deserialize)]
pub struct RoomAsked {
  chat: i64,
  #[serde(default)]
  awaiting: bool,
}

/// A room's briefings, or only its summaries still awaiting their line.
pub async fn briefings(
  State(served): State<Served>,
  Query(asked): Query<RoomAsked>,
) -> Result<Json<Value>, Refused> {
  let rows = briefings::of_room(&served.context.db, asked.chat, asked.awaiting)
    .await
    .map_err(Refused::failed)?;
  let listed: Vec<Value> = rows
    .into_iter()
    .map(|row| {
      json!({
        "run": row.run_id.to_string(),
        "chat": row.chat_id,
        "message": row.message_id,
        "voice_message": row.voice_message_id,
        "kind": row.kind,
        "title": row.title,
        "awaits_line": row.awaits_line,
        "started_at": row.started_at,
        "finished": row.finished,
      })
    })
    .collect();

  Ok(Json(json!({ "briefings": listed })))
}

fn run_of(run: &str) -> Result<i64, Refused> {
  run.parse().map_err(|_| {
    Refused::new(StatusCode::NOT_FOUND, "no_briefing", format!("there is no briefing of run {run}"))
  })
}

#[derive(Debug, Deserialize)]
pub struct Line {
  message: i64,
}

/// The line a summary's round sent, named under it.
pub async fn line(
  State(served): State<Served>,
  Path(run): Path<String>,
  Json(asked): Json<Line>,
) -> Result<StatusCode, Refused> {
  let run_id = run_of(&run)?;

  if briefings::tie_line(&served.context.db, run_id, asked.message)
    .await
    .map_err(Refused::failed)?
  {
    Ok(StatusCode::NO_CONTENT)
  } else {
    Err(Refused::new(
      StatusCode::NOT_FOUND,
      "no_briefing",
      format!("there is no briefing of run {run}"),
    ))
  }
}

/// A run's kept sound, answering ranges.
pub async fn sound(
  State(served): State<Served>,
  Path(run): Path<String>,
  request: Request,
) -> Result<Response, Refused> {
  let run_id = run_of(&run)?;
  let kept = kept_sound(&served.context.settings, run_id);

  if !kept.is_file() {
    return Err(Refused::new(
      StatusCode::NOT_FOUND,
      "no_sound",
      format!("no sound is kept for run {run}"),
    ));
  }

  let served = ServeFile::new_with_mime(&kept, &"audio/ogg".parse().expect("a mime type"))
    .oneshot(request)
    .await
    .map_err(Refused::failed)?;

  Ok(served.into_response())
}

#[derive(Debug, Deserialize)]
pub struct Stopped {
  chat: i64,
  message: i64,
}

/// A listener stopped a briefing: every listener playing it stops.
pub async fn stop(State(served): State<Served>, Json(asked): Json<Stopped>) -> StatusCode {
  served
    .hub
    .file("briefing-stopped", json!({ "chat": asked.chat, "message": asked.message }).to_string());

  StatusCode::NO_CONTENT
}

/// A briefing a runner put in the room, filed for the listeners.
pub async fn heard(
  State(served): State<Served>,
  Json(frame): Json<runner::Frame>,
) -> Result<StatusCode, Refused> {
  let data = serde_json::to_string(&frame).map_err(Refused::failed)?;

  served.hub.file("briefing", data);

  Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct Resume {
  after: Option<u64>,
}

fn event_of(frame: Frame) -> Result<Event, Infallible> {
  Ok(Event::default().id(frame.id.to_string()).event(frame.event).data(frame.data))
}

/// The listener stream: the frames kept after the cursor a reconnecting listener names, then every frame as it is
/// filed, with a keep frame every five seconds.
pub async fn events(
  State(served): State<Served>,
  headers: HeaderMap,
  Query(resume): Query<Resume>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
  let cursor = headers
    .get("last-event-id")
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.parse().ok())
    .or(resume.after);
  let (replayed, receiver) = served.hub.after(cursor);
  let live = stream::unfold(receiver, |mut receiver| async move {
    loop {
      match receiver.recv().await {
        Ok(frame) => return Some((frame, receiver)),
        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
      }
    }
  });
  let frames = stream::iter(replayed).chain(live).map(event_of);

  Sse::new(frames).keep_alive(KeepAlive::new().interval(KEEP_EVERY))
}

#[derive(Debug, Deserialize)]
pub struct ScriptAsked {
  text: String,
  #[serde(default)]
  full: bool,
}

/// A line's script reading, as the voice's own rules make it and no gate is asked: the script, each Korean stretch
/// as the reading gate would be shown it with its spans, and for a full reading the view the polishing gate is shown.
pub async fn script_reading(
  State(served): State<Served>,
  Json(asked): Json<ScriptAsked>,
) -> Result<Json<Value>, Refused> {
  let names = voices::said_names(&served.context.db).await.map_err(Refused::failed)?;
  let script = spoken::spoken_script(&asked.text);
  let stretches: Vec<Value> = reading::voice_stretches(&script)
    .into_iter()
    .filter(|stretch| stretch.language == Language::Ko)
    .map(|stretch| {
      let named = reading::say_names(&stretch.text, stretch.language, &names);
      let said = figures::say_figures(&named);
      let spans = reading::reading_spans(&said);
      let listed: Vec<Value> = spans
        .iter()
        .enumerate()
        .map(|(at, span)| json!({ "n": at + 1, "text": span.text, "code": span.code }))
        .collect();

      json!({ "text": said, "marked": reading::marked_line(&said, &spans), "spans": listed })
    })
    .collect();
  let polish = asked.full.then(|| {
    let (view, codes) = polish::polish_view(&script);

    json!({ "view": view, "codes": codes })
  });

  Ok(Json(json!({ "script": script, "stretches": stretches, "polish": polish })))
}

/// The voice's own policy values.
pub async fn policy() -> Json<Value> {
  Json(serde_json::to_value(&*POLICY).unwrap_or(Value::Null))
}
