//! The generate entry (docs/boundary-api.md, 「The hosted tool」): its arguments, and a sound of the words made
//! against an engine standing in on the loopback: taken in as a synthesis, read until it is done, its sound written
//! where the caller names, a voice no list holds refused with the served voices named, a failed synthesis refused
//! with its failure, and one still waiting at its bound let go.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path as RoutePath, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use voice_servers_tool::{GenerateAsked, Pace, generate_at, read_generate_arguments, target};

/// The sound the stand-in engine answers.
const SOUND: &[u8] = b"OggS a sound";

/// How the stand-in engine's synthesis ends when it is read.
#[derive(Clone, Copy)]
enum Ends {
  Done,
  Failed,
  Stays,
}

/// What the stand-in engine was asked and how it answers.
struct Engine {
  ends: Ends,
  taken: Mutex<Vec<Value>>,
}

fn synthesis(state: &str, ends: Ends) -> Value {
  json!({
    "id": 7,
    "voiceName": "alpha",
    "text": "hello there",
    "spoken": if state == "done" { json!("hello there, said") } else { Value::Null },
    "emotion": null,
    "key": null,
    "state": state,
    "wait": null,
    "waitingOn": if state == "waiting" { json!("the card is being asked of the master") } else { Value::Null },
    "failure": if matches!(ends, Ends::Failed) && state == "failed" { json!("the node did not answer") } else { Value::Null },
    "durationMs": if state == "done" { json!(1500) } else { Value::Null },
    "createdAt": 0,
  })
}

async fn take(State(engine): State<Arc<Engine>>, Json(body): Json<Value>) -> impl IntoResponse {
  if body["voiceName"] == "nobody" {
    return (
      StatusCode::NOT_FOUND,
      Json(json!({ "code": "not-found", "error": "no voice is named nobody" })),
    );
  }

  engine.taken.lock().unwrap().push(body);

  (StatusCode::CREATED, Json(synthesis("waiting", engine.ends)))
}

async fn read(State(engine): State<Arc<Engine>>, RoutePath(id): RoutePath<i64>) -> Json<Value> {
  assert_eq!(id, 7);

  Json(match engine.ends {
    Ends::Done => synthesis("done", engine.ends),
    Ends::Failed => synthesis("failed", engine.ends),
    Ends::Stays => synthesis("waiting", engine.ends),
  })
}

async fn sound() -> &'static [u8] {
  SOUND
}

async fn voices() -> Json<Value> {
  let voice = |name: &str, served: bool| {
    json!({
      "name": name, "language": "ko", "checkpoint": "c", "engine": "fish-s2-pro", "served": served,
      "emotions": [], "changedAt": 0,
    })
  };

  Json(
    json!({ "items": [voice("alpha", true), voice("beta", false), voice("gamma", true)], "more": false }),
  )
}

/// A scratch folder of the test's own, its settings naming the stand-in engine at `at` with a token.
fn settings_at(name: &str, at: &str) -> (PathBuf, PathBuf) {
  let root =
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("voice-generate-{name}-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&root);

  std::fs::create_dir_all(root.join("env")).unwrap();
  std::fs::write(root.join("env").join("voice.token"), "a-token\n").unwrap();

  let settings = root.join("settings.toml");

  std::fs::write(
    &settings,
    format!(
      r#"listen = "127.0.0.1:0"
address = "http://{at}"
token_file = "voice.token"
database = "postgres:///unused"
data_root = "{}"

[units.node]
address = "http://[::1]:9"

[units.arbiter]
address = "http://[::1]:9"

[units.web]
address = "http://[::1]:9"
"#,
      root.display()
    ),
  )
  .unwrap();

  (root, settings)
}

/// The stand-in engine served on the loopback for the test's runtime, its synthesis ending as `ends` says.
async fn engine(ends: Ends) -> (Arc<Engine>, String) {
  let engine = Arc::new(Engine {
    ends,
    taken: Mutex::default(),
  });
  let app = Router::new()
    .route("/syntheses", post(take))
    .route("/syntheses/{id}", get(read))
    .route("/syntheses/{id}/sound", get(sound))
    .route("/voices", get(voices))
    .with_state(Arc::clone(&engine));
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let at = listener.local_addr().unwrap().to_string();

  tokio::spawn(async move { axum::serve(listener, app).await });

  (engine, at)
}

fn asked(voice_name: &str, path: Option<&Path>) -> GenerateAsked {
  read_generate_arguments(&json!({
    "voiceName": voice_name,
    "text": "hello there",
    "emotion": "calm",
    "path": path.map(|path| path.to_string_lossy().into_owned()),
  }))
  .unwrap()
}

// Read at once, and waited on for as long as a call may be.
const AT_ONCE: Pace = Pace {
  every: Duration::ZERO,
  within: Duration::from_secs(600),
};

fn text(answer: &voice_servers_tool::Answer) -> String {
  answer.plain()
}

// "the arguments are a voice and words that hold something, and an absolute path where one is given"
#[test]
fn the_arguments_are_read() {
  let asked = read_generate_arguments(&json!({ "voiceName": "alpha", "text": "hi" })).unwrap();

  assert_eq!(asked.voice_name, "alpha");
  assert_eq!(asked.path, None);
  assert_eq!(target(&asked, 7), std::env::temp_dir().join("bmo-voice").join("generated-7.ogg"));

  for (arguments, said) in [
    (json!({ "text": "hi" }), "voiceName"),
    (json!({ "voiceName": " ", "text": "hi" }), "voice to speak in"),
    (json!({ "voiceName": "alpha", "text": " " }), "something to say"),
    (json!({ "voiceName": "alpha", "text": "hi", "path": "line.ogg" }), "absolute path"),
    (json!({ "voiceName": "alpha", "text": "hi", "chatId": 1 }), "chatId"),
  ] {
    let refusal = read_generate_arguments(&arguments).unwrap_err();

    assert!(refusal.contains(said), "{refusal}");
  }
}

// "the words are taken in as a synthesis tied to nothing, read until done, and its sound written where named"
#[tokio::test]
async fn a_sound_of_the_words_is_written_where_named() {
  let (engine, at) = engine(Ends::Done).await;
  let (root, settings) = settings_at("done", &at);
  let path = root.join("out").join("hello.ogg");

  let answer = generate_at(&settings, &asked("alpha", Some(&path)), AT_ONCE).await;
  let said = text(&answer);

  assert!(!answer.is_error, "{said}");
  assert_eq!(std::fs::read(&path).unwrap(), SOUND);
  assert!(said.contains(&path.display().to_string()), "{said}");
  assert!(said.contains("1.5 seconds in the voice alpha, synthesis 7"), "{said}");
  assert!(said.contains("The voice said: hello there, said"), "{said}");

  let taken = engine.taken.lock().unwrap().clone();

  assert_eq!(
    taken,
    [json!({
      "voiceName": "alpha", "text": "hello there", "spoken": null, "language": null, "emotion": "calm",
      "style": null, "seed": null, "key": null,
    })]
  );

  std::fs::remove_dir_all(&root).unwrap();
}

// "a voice no list holds is refused with the served voices named, and nothing is written"
#[tokio::test]
async fn an_unknown_voice_names_the_served_ones() {
  let (_engine, at) = engine(Ends::Done).await;
  let (root, settings) = settings_at("unknown", &at);
  let path = root.join("nobody.ogg");

  let answer = generate_at(&settings, &asked("nobody", Some(&path)), AT_ONCE).await;
  let said = text(&answer);

  assert!(answer.is_error, "{said}");
  assert_eq!(said, "no voice is named nobody; the served voices are alpha, gamma");
  assert!(!path.exists());

  std::fs::remove_dir_all(&root).unwrap();
}

// "a failed synthesis is refused with its failure, and one still waiting at its bound is let go saying what it waits on"
#[tokio::test]
async fn a_synthesis_with_no_sound_is_refused() {
  for (ends, name, pace, expected) in [
    (Ends::Failed, "failed", AT_ONCE, "synthesis 7 failed: the node did not answer"),
    (
      Ends::Stays,
      "bound",
      Pace {
        every: Duration::ZERO,
        within: Duration::ZERO,
      },
      "synthesis 7 is waiting and has not ended, waiting on: the card is being asked of the master",
    ),
  ] {
    let (_engine, at) = engine(ends).await;
    let (root, settings) = settings_at(name, &at);
    let path = root.join("line.ogg");

    let answer = generate_at(&settings, &asked("alpha", Some(&path)), pace).await;

    assert!(answer.is_error);
    assert_eq!(text(&answer), expected);
    assert!(!path.exists());

    std::fs::remove_dir_all(&root).unwrap();
  }
}
