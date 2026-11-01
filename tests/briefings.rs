//! The briefings (`docs/boundary-api.md`, 「Briefings」): a line spoken for a room is taken in at once and kept with
//! its room and its line, its end is told on the event stream, a summary's line is named after it, a full reading
//! names its line and keeps it, and the page narrows by room, line and kind; and the synthesis a line is spoken as
//! waits for its card, saying why, and gives it back as it ends; and a caller's key takes a line in once.

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
/// Spoken words a Korean voice says as they stand, given beside each line so none waits on a derivation.
const SPOKEN: &str = "백업이 끝났습니다.";

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

/// The engine's routes on the scratch store, with one voice whose folder stands outside the voices root, nowhere a node
/// holds, so every line in it fails to be spoken; one, `voice-b`, under the voices root, whose lines ask the master
/// `master` names; and one, `voice-c`, whose folder declares no engine, as a retired voice's does.
async fn engine(scratch: &Scratch, master: Option<std::net::SocketAddr>) -> axum::Router {
  let master = master.map_or_else(|| "127.0.0.1:9".to_owned(), |at| at.to_string());
  let settings_path = scratch.root.join("env").join("voice.toml");
  let settings = format!(
    r#"listen = "127.0.0.1:0"
address = "http://127.0.0.1:0"
token_file = "voice.token"
database = "{}"
data_root = "{}"

[units.node]
address = "http://127.0.0.1:9"

[units.arbiter]
address = "http://{master}"

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
  let voices = settings.voices_folder();
  let outside = scratch.root.join("elsewhere").join("voice-a");

  for folder in [&outside, &voices.join("voice-b")] {
    std::fs::create_dir_all(folder).expect("the voice folder is made");
    std::fs::write(folder.join("voice.toml"), "engine = \"fish-s2-pro\"\n").expect("the voice is declared");
  }

  std::fs::create_dir_all(voices.join("voice-c")).expect("the retired voice's folder is made");
  db.execute_unprepared(&format!(
    r#"INSERT INTO "voices" ("name", "checkpoint", "reference", "cfg", "steps", "set_at")
       VALUES ('voice-a', '{}', '', 0, 0, 0), ('voice-c', '{}', 'reference/voice-c.wav', 2.0, 10, 0)"#,
    outside.display(),
    voices.join("voice-c").display()
  ))
  .await
  .expect("the voices are written");

  db.execute_unprepared(&format!(
    r#"INSERT INTO "voices" ("name", "checkpoint", "reference", "cfg", "steps", "set_at")
       VALUES ('voice-b', '{}', '{}', 2.0, 10, 0)"#,
    voices.join("voice-b").display(),
    voices.join("reference/voice-b.wav").display()
  ))
  .await
  .expect("the voice under the root is written");

  let units = Units::new(&settings).expect("the units are named");
  let context = Context {
    settings,
    db: db.clone(),
    units,
    waits: Default::default(),
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
  let engine = engine(&scratch, None).await;
  let (status, briefing) = call(
    &engine,
    "POST",
    "/briefings",
    Some(
      json!({ "voiceName": "voice-a", "text": "The backup finished.", "spoken": SPOKEN, "chatId": ROOM, "kind": "summary",
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
  let engine = engine(&scratch, None).await;
  let asked = |message: Value| {
    json!({ "voiceName": "voice-a", "text": "The whole line, read out.", "spoken": SPOKEN, "chatId": ROOM, "messageId": message,
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
    Some(json!({ "voiceName": "voice-a", "text": "Another room.", "spoken": SPOKEN, "chatId": 9, "kind": "summary" })),
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

  assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
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

#[tokio::test]
async fn a_retired_voice_is_listed_as_such_and_never_spoken() {
  let scratch = Scratch::new().await;
  let engine = engine(&scratch, None).await;
  let (_, retired) = call(&engine, "GET", "/voices/voice-c", None).await;
  let (_, served) = call(&engine, "GET", "/voices/voice-b", None).await;

  assert_eq!(
    (retired["engine"].as_str(), retired["served"].as_bool()),
    (Some("voxcpm2"), Some(false))
  );
  assert_eq!(
    (served["engine"].as_str(), served["served"].as_bool()),
    (Some("fish-s2-pro"), Some(true))
  );

  for (path, body) in [
    ("/syntheses", json!({ "voiceName": "voice-c", "text": "A line." })),
    (
      "/briefings",
      json!({ "voiceName": "voice-c", "text": "A line.", "chatId": ROOM, "kind": "summary" }),
    ),
  ] {
    let (status, refused) = call(&engine, "POST", path, Some(body)).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{path}: {refused}");
    assert_eq!(refused["code"], "retired-voice", "{path}: {refused}");
  }
}

/// A stand-in master: a demand waits, its reads saying why, until the test lets it be granted on a node nothing
/// reaches; every demand given back is kept.
#[derive(Default)]
struct Line {
  open: std::sync::atomic::AtomicBool,
  given: std::sync::Mutex<Vec<i64>>,
}

const IN_USE: &str = "Every card of voice is in use: song.";

fn line_demand(line: &Line) -> Value {
  if line.open.load(Ordering::SeqCst) {
    json!({ "id": 9, "state": "granted", "taken": null, "reason": null,
      "cards": [{ "nodeName": "node-a" }] })
  } else {
    json!({ "id": 9, "state": "waiting", "taken": null, "cards": [],
      "reason": { "code": "in-use", "text": IN_USE } })
  }
}

async fn line_master(line: Arc<Line>) -> std::net::SocketAddr {
  use axum::extract::{Path, State};
  use axum::routing::{get, post};

  let router = axum::Router::new()
    .route(
      "/demands",
      post(|State(line): State<Arc<Line>>| async move {
        (StatusCode::ACCEPTED, axum::Json(line_demand(&line)))
      })
      .get(|State(line): State<Arc<Line>>| async move {
        axum::Json(json!({ "items": [line_demand(&line)], "more": false }))
      }),
    )
    .route(
      "/demands/{id}",
      get(|State(line): State<Arc<Line>>| async move {
        tokio::time::sleep(Duration::from_millis(20)).await;

        axum::Json(line_demand(&line))
      })
      .delete(|State(line): State<Arc<Line>>, Path(id): Path<i64>| async move {
        line.given.lock().unwrap().push(id);

        StatusCode::NO_CONTENT
      }),
    )
    .with_state(line);
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a port");
  let at = listener.local_addr().expect("its address");

  tokio::spawn(async move { axum::serve(listener, router).await });

  at
}

#[tokio::test]
async fn a_synthesis_waits_for_its_card_says_why_and_gives_it_back_as_it_ends() {
  let scratch = Scratch::new().await;
  let line = Arc::new(Line::default());
  let engine = engine(&scratch, Some(line_master(Arc::clone(&line)).await)).await;
  let (status, made) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-b", "text": "A short line.", "spoken": SPOKEN })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{made}");

  let id = made["id"].as_i64().expect("an id");
  let mut read = Value::Null;

  for _ in 0..400 {
    (_, read) = call(&engine, "GET", &format!("/syntheses/{id}"), None).await;

    if read["waitingOn"].is_string() {
      break;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  assert_eq!(read["state"], "waiting", "{read}");
  assert_eq!(read["waitingOn"], IN_USE, "{read}");

  line.open.store(true, Ordering::SeqCst);

  for _ in 0..400 {
    (_, read) = call(&engine, "GET", &format!("/syntheses/{id}"), None).await;

    if read["state"] == "failed" {
      break;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  assert_eq!(read["state"], "failed", "the node granted is out of reach: {read}");
  assert_eq!(read["waitingOn"], Value::Null);
  assert_eq!(*line.given.lock().unwrap(), [9], "the card is given back as the line ends");
}

#[tokio::test]
async fn three_hundred_waiting_lines_share_one_runtime_and_one_read_of_the_master() {
  let scratch = Scratch::new().await;
  let line = Arc::new(Line::default());
  let engine = engine(&scratch, Some(line_master(Arc::clone(&line)).await)).await;
  let open_files = || std::fs::read_dir("/proc/self/fd").map_or(0, Iterator::count);
  let before = open_files();

  for _ in 0..300 {
    let (status, made) = call(
      &engine,
      "POST",
      "/syntheses",
      Some(json!({ "voiceName": "voice-b", "text": "A short line.", "spoken": SPOKEN })),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{made}");
  }

  // Every line asks its demand and waits on it; the reads of the master go on meanwhile.
  tokio::time::sleep(Duration::from_secs(3)).await;

  let (_, read) = call(&engine, "GET", "/syntheses/300", None).await;
  let during = open_files();

  eprintln!("open files: {before} before 300 waiting lines, {during} while they wait");
  assert_eq!(read["waitingOn"], IN_USE, "the line waits on its card, told why: {read}");
  assert!(
    during < before + 200,
    "300 waiting lines hold {} files beyond the {before} before them",
    during - before
  );
}

#[tokio::test]
async fn a_key_takes_a_line_in_once_until_it_fails() {
  let scratch = Scratch::new().await;
  let engine = engine(&scratch, None).await;
  let line =
    |text: &str| json!({ "voiceName": "voice-b", "text": text, "spoken": SPOKEN, "key": "research:7:ko" });
  let (status, first) = call(&engine, "POST", "/syntheses", Some(line("The first asking."))).await;

  assert_eq!(status, StatusCode::CREATED, "{first}");
  assert_eq!(first["key"], "research:7:ko");

  // Asked again with the same key while the first waits: the first answers, whatever else the body says.
  let (status, again) = call(&engine, "POST", "/syntheses", Some(line("Another text."))).await;

  assert_eq!(status, StatusCode::OK, "{again}");
  assert_eq!(again["id"], first["id"]);
  assert_eq!(again["text"], "The first asking.");

  // Once the one holding the key has failed, the key takes a new one in.
  let mut store = PgConnection::connect(&address_of(&scratch.name)).await.expect("the store answers");
  store
    .execute(
      format!(r#"UPDATE "voice_syntheses" SET "state" = 'failed' WHERE "id" = {}"#, first["id"]).as_str(),
    )
    .await
    .expect("the synthesis is failed");
  store.close().await.expect("the store connection closes");

  let (status, renewed) = call(&engine, "POST", "/syntheses", Some(line("After the failure."))).await;

  assert_eq!(status, StatusCode::CREATED, "{renewed}");
  assert_ne!(renewed["id"], first["id"]);

  // Two at once take in one between them.
  let (one, two) = tokio::join!(
    call(
      &engine,
      "POST",
      "/syntheses",
      Some(json!({ "voiceName": "voice-b", "text": "x", "spoken": SPOKEN, "key": "k2" }))
    ),
    call(
      &engine,
      "POST",
      "/syntheses",
      Some(json!({ "voiceName": "voice-b", "text": "x", "spoken": SPOKEN, "key": "k2" }))
    ),
  );

  assert_eq!(one.1["id"], two.1["id"], "{} {}", one.1, two.1);
  assert_eq!([one.0, two.0].iter().filter(|status| **status == StatusCode::CREATED).count(), 1);

  let (status, refused) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-b", "text": "x", "spoken": SPOKEN, "key": "  " })),
  )
  .await;

  assert_eq!((status, refused["code"].as_str()), (StatusCode::BAD_REQUEST, Some("bad-request")));
}

#[tokio::test]
async fn a_key_takes_a_briefing_in_once_and_a_plain_synthesis_key_is_a_conflict() {
  let scratch = Scratch::new().await;
  let engine = engine(&scratch, None).await;
  let briefing = || {
    json!({ "voiceName": "voice-b", "text": "The backup finished.", "spoken": SPOKEN, "chatId": ROOM,
            "kind": "summary", "key": "research:9:ko" })
  };
  let (status, first) = call(&engine, "POST", "/briefings", Some(briefing())).await;

  assert_eq!(status, StatusCode::CREATED, "{first}");

  let (status, again) = call(&engine, "POST", "/briefings", Some(briefing())).await;

  assert_eq!(status, StatusCode::OK, "{again}");
  assert_eq!(again["id"], first["id"]);

  let (status, _) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-b", "text": "x", "spoken": SPOKEN, "key": "plain" })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED);

  let (status, refused) = call(
    &engine,
    "POST",
    "/briefings",
    Some(
      json!({ "voiceName": "voice-b", "text": "x", "spoken": SPOKEN, "chatId": ROOM, "kind": "summary",
                 "key": "plain" }),
    ),
  )
  .await;

  assert_eq!(
    (status, refused["code"].as_str()),
    (StatusCode::CONFLICT, Some("conflict")),
    "{refused}"
  );
}
