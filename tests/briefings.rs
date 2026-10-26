//! The briefings (`docs/boundary-api.md`, 「Briefings」): a line spoken for a room is taken in at once and kept with
//! its room and its line, its end is told on the event stream, a summary's line is named after it, a full reading
//! names its line and keeps it, and the page narrows by room, line and kind.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use bmo_voice::context::Context;
use bmo_voice::events::Events;
use bmo_voice::http::{Served, router};
use bmo_voice::settings::Settings;
use bmo_voice::units::Units;
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};
use tower::ServiceExt;

static MADE: AtomicU32 = AtomicU32::new(0);

const TOKEN: &str = "a-test-token";
const ROOM: i64 = -500;

fn server() -> String {
  std::env::var("BMO_TEST_POSTGRES")
    .ok()
    .filter(|named| !named.is_empty())
    .unwrap_or_else(|| "postgres:///bmo_test?host=/var/run/postgresql".into())
}

fn address_of(database: &str) -> String {
  let mut address = url::Url::parse(&server()).expect("the server's address reads");
  address.set_path(&format!("/{database}"));

  address.to_string()
}

/// A database and a data root of this test's own, made empty and dropped when the test ends.
struct Scratch {
  name: String,
  root: std::path::PathBuf,
}

impl Scratch {
  async fn new() -> Scratch {
    let made = MADE.fetch_add(1, Ordering::SeqCst);
    let name = format!("bmo_voice_briefings_test_{}_{made}", std::process::id());
    let mut admin = PgConnection::connect(&address_of("postgres")).await.expect("the server answers");
    let _ = admin.execute(format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)").as_str()).await;
    admin
      .execute(format!("CREATE DATABASE \"{name}\" TEMPLATE template0").as_str())
      .await
      .expect("the database is made");
    admin.close().await.expect("the server connection closes");

    let root = std::env::temp_dir().join(&name);

    std::fs::create_dir_all(root.join("env")).expect("the data root is made");

    Scratch { name, root }
  }
}

impl Drop for Scratch {
  fn drop(&mut self) {
    let name = std::mem::take(&mut self.name);
    let _ = std::fs::remove_dir_all(&self.root);
    let _ = std::thread::spawn(move || {
      let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
      runtime.block_on(async move {
        if let Ok(mut admin) = PgConnection::connect(&address_of("postgres")).await {
          let _ = admin.execute(format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)").as_str()).await;
          let _ = admin.close().await;
        }
      });
    })
    .join();
  }
}

/// The engine's routes on the scratch store, with one voice and no voice node, so every line fails to be spoken.
async fn engine(scratch: &Scratch) -> axum::Router {
  let settings_path = scratch.root.join("env").join("voice.toml");
  let settings = format!(
    r#"listen = "127.0.0.1:0"
address = "http://127.0.0.1:0"
token_file = "voice.token"
database = "{}"
data_root = "{}"

[units.node]
address = "http://127.0.0.1:9"

[units.web]
address = "http://127.0.0.1:9"
"#,
    address_of(&scratch.name),
    scratch.root.display()
  );

  std::fs::write(&settings_path, settings).expect("the settings are written");

  let settings = Settings::read(&settings_path).expect("the settings read");
  let db = bmo_voice::store::connect(&settings.database, 4).await.expect("the store opens");

  bmo_voice::store::migrate(&db).await.expect("the store migrates");
  db.execute_unprepared(
    r#"INSERT INTO "voices" ("name", "checkpoint", "reference", "cfg", "steps", "set_at")
       VALUES ('voice-a', 'voice-a', 'reference/voice-a.wav', 2.0, 10, 0)"#,
  )
  .await
  .expect("the voice is written");

  let units = Units::new(&settings).expect("the units are named");
  let context = Context {
    settings,
    db: db.clone(),
    units,
  };

  router(Served {
    context: Arc::new(context),
    events: Events::new(db),
    token: Some(Arc::new(TOKEN.to_owned())),
  })
}

use sea_orm::ConnectionTrait;

async fn call(engine: &axum::Router, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
  let request = Request::builder()
    .method(method)
    .uri(path)
    .header("authorization", format!("Bearer {TOKEN}"))
    .header("accept", "application/json")
    .header("content-type", "application/json")
    .body(body.map_or_else(Body::empty, |body| Body::from(body.to_string())))
    .expect("a request");
  let answer = engine.clone().oneshot(request).await.expect("an answer");
  let status = answer.status();
  let bytes = axum::body::to_bytes(answer.into_body(), 1 << 20).await.expect("a body");

  (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

async fn ended(engine: &axum::Router, id: i64) -> Value {
  for _ in 0..400 {
    let (_, briefing) = call(engine, "GET", &format!("/briefings/{id}"), None).await;

    if briefing["finishedAt"].is_i64() {
      return briefing;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  panic!("briefing {id} never ended");
}

#[tokio::test]
async fn a_summary_is_taken_in_at_once_its_end_is_told_and_its_line_is_named_after_it() {
  let scratch = Scratch::new().await;
  let engine = engine(&scratch).await;
  let (status, briefing) = call(
    &engine,
    "POST",
    "/briefings",
    Some(
      json!({ "voiceName": "voice-a", "text": "The backup finished.", "chatId": ROOM, "kind": "summary",
      "title": "The backup", "plays": false }),
    ),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{briefing}");
  assert_eq!(briefing["state"], "waiting");
  assert_eq!(briefing["chatId"], ROOM);
  assert_eq!(briefing["messageId"], Value::Null);
  assert_eq!(briefing["awaitsLine"], true);
  assert_eq!(briefing["plays"], false);
  assert_eq!(briefing["title"], "The backup");
  assert!(briefing["synthesisId"].is_i64());

  let id = briefing["id"].as_i64().expect("an id");
  let over = ended(&engine, id).await;

  assert_eq!(over["state"], "failed", "no voice node speaks it: {over}");

  let (status, _) = call(&engine, "GET", &format!("/briefings/{id}/sound"), None).await;

  assert_eq!(status, StatusCode::NOT_FOUND);

  let (status, named) =
    call(&engine, "POST", &format!("/briefings/{id}/line"), Some(json!({ "messageId": 7 }))).await;

  assert_eq!(status, StatusCode::OK, "{named}");
  assert_eq!(named["messageId"], 7);
  assert_eq!(named["awaitsLine"], false);

  let (_, page) =
    call(&engine, "GET", "/events?kinds=briefing.created,briefing.ended,briefing.changed", None).await;
  let kinds: Vec<&str> = page["items"]
    .as_array()
    .expect("a page")
    .iter()
    .filter_map(|event| event["kind"].as_str())
    .collect();

  assert_eq!(kinds, ["briefing.created", "briefing.ended", "briefing.changed"]);
}

#[tokio::test]
async fn a_full_reading_names_its_line_keeps_it_and_the_page_narrows_to_it() {
  let scratch = Scratch::new().await;
  let engine = engine(&scratch).await;
  let asked = |message: Value| {
    json!({ "voiceName": "voice-a", "text": "The whole line, read out.", "chatId": ROOM, "messageId": message,
      "kind": "full" })
  };
  let (status, _) = call(&engine, "POST", "/briefings", Some(asked(Value::Null))).await;

  assert_eq!(status, StatusCode::BAD_REQUEST, "a full reading names its line");

  let (status, full) = call(&engine, "POST", "/briefings", Some(asked(json!(11)))).await;

  assert_eq!(status, StatusCode::CREATED, "{full}");
  assert_eq!(full["awaitsLine"], false);
  assert_eq!(full["plays"], true);

  let id = full["id"].as_i64().expect("an id");
  let (status, refused) =
    call(&engine, "POST", &format!("/briefings/{id}/line"), Some(json!({ "messageId": 12 }))).await;

  assert_eq!(status, StatusCode::CONFLICT);
  assert_eq!(refused["code"], "conflict");

  let (status, _) = call(
    &engine,
    "POST",
    "/briefings",
    Some(json!({ "voiceName": "voice-a", "text": "Another room.", "chatId": 9, "kind": "summary" })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED);

  let (_, page) =
    call(&engine, "GET", &format!("/briefings?chatId={ROOM}&messageId=11&kind=full"), None).await;
  let items = page["items"].as_array().expect("a page");

  assert_eq!(items.len(), 1, "{page}");
  assert_eq!(items[0]["id"], id);

  let (_, all) = call(&engine, "GET", "/briefings?limit=1", None).await;

  assert_eq!(all["more"], true);
  assert!(all["items"][0]["id"].as_i64() > Some(id), "newest first: {all}");

  let (status, refused) = call(
    &engine,
    "POST",
    "/briefings",
    Some(json!({ "voiceName": "voice-a", "text": "x", "chatId": ROOM, "kind": "summary", "emotion": "no-such" })),
  )
  .await;

  assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
  assert_eq!(refused["code"], "unknown-emotion");

  let (status, _) = call(
    &engine,
    "POST",
    "/briefings",
    Some(json!({ "voiceName": "no-voice", "text": "x", "chatId": ROOM, "kind": "summary" })),
  )
  .await;

  assert_eq!(status, StatusCode::NOT_FOUND);
}
