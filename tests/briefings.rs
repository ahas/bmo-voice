//! The briefings (`docs/boundary-api.md`, 「Briefings」): a line spoken for a room is taken in at once and kept with
//! its room and its line, its end is told on the event stream, a summary's line is named after it, a full reading
//! names its line and keeps it, and the page narrows by room, line and kind; and the synthesis a line is spoken as
//! waits for its card, saying why, and gives it back as it ends; a caller's key takes a line in once; and a stop ends
//! every briefing of a room, or of every room, that has not ended, gives back its card and tells it; and switching
//! the briefings off ends a waiting line and a speaking one alike, neither of which plays or is spoken afterwards.

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
  router(served(scratch, master, None).await)
}

/// What the engine's routes stand on, as `engine` builds it, its node service the one `node` names.
async fn served(
  scratch: &Scratch,
  master: Option<std::net::SocketAddr>,
  node: Option<std::net::SocketAddr>,
) -> Served {
  let master = master.map_or_else(|| "[::1]:9".to_owned(), |at| at.to_string());
  let node = node.map_or_else(|| "[::1]:9".to_owned(), |at| at.to_string());
  let settings_path = scratch.root.join("env").join("voice.toml");
  let settings = format!(
    r#"listen = "127.0.0.1:0"
address = "http://127.0.0.1:0"
token_file = "voice.token"
database = "{}"
data_root = "{}"

[units.node]
address = "http://{node}"

[units.arbiter]
address = "http://{master}"

[units.web]
address = "http://[::1]:9"
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
    stops: Default::default(),
  };

  Served {
    context: Arc::new(context),
    events: Events::new(db),
    token: Some(Arc::new(TOKEN.to_owned())),
  }
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
/// reaches; every demand given back is kept. Its one demand is asked with the first synthesis's work key.
#[derive(Default)]
struct Line {
  open: std::sync::atomic::AtomicBool,
  given: std::sync::Mutex<Vec<i64>>,
}

const IN_USE: &str = "Every card of voice is in use: song.";

fn line_demand(line: &Line) -> Value {
  if line.open.load(Ordering::SeqCst) {
    json!({ "id": 9, "state": "granted", "taken": null, "reason": null, "workKey": "voice:synthesis:1",
      "cards": [{ "nodeName": "node-a" }] })
  } else {
    json!({ "id": 9, "state": "waiting", "taken": null, "cards": [], "workKey": "voice:synthesis:1",
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
async fn a_line_is_taken_in_within_its_bounds_and_refused_past_them() {
  let scratch = Scratch::new().await;
  let engine = engine(&scratch, None).await;
  let words = |count: usize| "가".repeat(count);
  let synthesis =
    |text: String, spoken: String| json!({ "voiceName": "voice-b", "text": text, "spoken": spoken });
  let briefing = |text: String, spoken: String| json!({ "voiceName": "voice-a", "text": text, "spoken": spoken, "chatId": ROOM, "kind": "summary" });

  // At the bounds, a synthesis and a briefing are taken in.
  let (status, taken) =
    call(&engine, "POST", "/syntheses", Some(synthesis(words(10_000), words(30_000)))).await;

  assert_eq!(status, StatusCode::CREATED, "{taken}");

  let (status, taken) =
    call(&engine, "POST", "/briefings", Some(briefing(words(10_000), words(30_000)))).await;

  assert_eq!(status, StatusCode::CREATED, "{taken}");

  // Past either, each is refused before anything is taken in.
  for (path, body) in [
    ("/syntheses", synthesis(words(10_001), SPOKEN.to_owned())),
    ("/syntheses", synthesis("x".to_owned(), words(30_001))),
    ("/briefings", briefing(words(10_001), SPOKEN.to_owned())),
    ("/briefings", briefing("x".to_owned(), words(30_001))),
  ] {
    let (status, refused) = call(&engine, "POST", path, Some(body)).await;

    assert_eq!(
      (status, refused["code"].as_str()),
      (StatusCode::BAD_REQUEST, Some("bad-request")),
      "{path} {refused}"
    );
  }
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

#[tokio::test]
async fn a_stop_ends_a_rooms_waiting_briefings_gives_back_their_card_and_tells_it() {
  let scratch = Scratch::new().await;
  let line = Arc::new(Line::default());
  let engine = engine(&scratch, Some(line_master(Arc::clone(&line)).await)).await;
  let briefing = |room: i64, key: &str| {
    json!({ "voiceName": "voice-b", "text": "The backup finished.", "spoken": SPOKEN, "chatId": room,
            "kind": "summary", "key": key })
  };
  let (_, here) = call(&engine, "POST", "/briefings", Some(briefing(ROOM, "stop:here"))).await;
  let (_, there) = call(&engine, "POST", "/briefings", Some(briefing(9, "stop:there"))).await;
  let (here_id, there_id) = (here["id"].as_i64().expect("an id"), there["id"].as_i64().expect("an id"));

  assert_eq!(here["synthesisId"], 1, "the stand-in master's demand names the first synthesis");

  let mut read = Value::Null;

  for _ in 0..400 {
    (_, read) = call(&engine, "GET", "/syntheses/1", None).await;

    if read["waitingOn"].is_string() {
      break;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  assert_eq!(read["waitingOn"], IN_USE, "the line waits on its card: {read}");

  let (status, stopped) = call(&engine, "POST", "/briefings/stop", Some(json!({ "chatId": ROOM }))).await;

  assert_eq!(status, StatusCode::OK, "{stopped}");
  assert_eq!(stopped["more"], false);
  assert_eq!(stopped["items"].as_array().map(Vec::len), Some(1), "{stopped}");
  assert_eq!(stopped["items"][0]["id"], here_id);
  assert_eq!(stopped["items"][0]["state"], "stopped");
  assert!(stopped["items"][0]["finishedAt"].is_i64());

  let (_, other) = call(&engine, "GET", &format!("/briefings/{there_id}"), None).await;

  assert_eq!(other["state"], "waiting", "another room's briefing goes on: {other}");

  for _ in 0..400 {
    if !line.given.lock().unwrap().is_empty() {
      break;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  assert_eq!(*line.given.lock().unwrap(), [9], "the stopped line's card is given back");

  let (_, synthesis) = call(&engine, "GET", "/syntheses/1", None).await;

  assert_eq!((synthesis["state"].as_str(), &synthesis["waitingOn"]), (Some("stopped"), &Value::Null));

  let (status, _) = call(&engine, "GET", &format!("/briefings/{here_id}/sound"), None).await;

  assert_eq!(status, StatusCode::NOT_FOUND);

  // Its key stays held: asked again, the stopped briefing answers and nothing is spoken anew.
  let (status, again) = call(&engine, "POST", "/briefings", Some(briefing(ROOM, "stop:here"))).await;

  assert_eq!(
    (status, &again["id"], again["state"].as_str()),
    (StatusCode::OK, &json!(here_id), Some("stopped"))
  );

  // A stop naming no room ends every room's, and a briefing already stopped is not stopped twice.
  let (_, everywhere) = call(&engine, "POST", "/briefings/stop", Some(json!({}))).await;
  let ids: Vec<&Value> =
    everywhere["items"].as_array().expect("a page").iter().map(|one| &one["id"]).collect();

  assert_eq!(ids, [&json!(there_id)], "{everywhere}");

  let (_, page) =
    call(&engine, "GET", "/events?kinds=synthesis.ended,briefing.ended,briefings.stopped", None).await;
  let told: Vec<(String, Value)> = page["items"]
    .as_array()
    .expect("a page")
    .iter()
    .map(|event| (event["kind"].as_str().unwrap_or_default().to_owned(), event["data"].clone()))
    .collect();
  let kinds: Vec<&str> = told.iter().map(|(kind, _)| kind.as_str()).collect();

  assert_eq!(
    kinds,
    [
      "synthesis.ended",
      "briefing.ended",
      "briefings.stopped",
      "synthesis.ended",
      "briefing.ended",
      "briefings.stopped"
    ]
  );
  assert_eq!(told[1].1["state"], "stopped");
  assert_eq!(told[2].1, json!({ "chatId": ROOM, "briefingIds": [here_id] }));
  assert_eq!(told[5].1, json!({ "chatId": null, "briefingIds": [there_id] }));
}

/// A stand-in master holding a demand for each line that asks: the one asked with `granted` as its work key is
/// granted a card on `node-a`, every other waits; each demand given back is kept.
struct Cards {
  granted: String,
  asked: std::sync::Mutex<Vec<(i64, String)>>,
  given: std::sync::Mutex<Vec<i64>>,
}

fn card_demand(cards: &Cards, id: i64, key: &str) -> Value {
  if key == cards.granted {
    json!({ "id": id, "state": "granted", "taken": null, "reason": null, "workKey": key,
      "cards": [{ "nodeName": "node-a" }] })
  } else {
    json!({ "id": id, "state": "waiting", "taken": null, "cards": [], "workKey": key,
      "reason": { "code": "in-use", "text": IN_USE } })
  }
}

// The demands the master still holds, those given back left out.
fn held_demands(cards: &Cards) -> Vec<Value> {
  let given = cards.given.lock().unwrap().clone();

  cards
    .asked
    .lock()
    .unwrap()
    .iter()
    .filter(|(id, _)| !given.contains(id))
    .map(|(id, key)| card_demand(cards, *id, key))
    .collect()
}

async fn cards_master(cards: Arc<Cards>) -> std::net::SocketAddr {
  use axum::extract::{Path, State};
  use axum::routing::{get, post};

  let router = axum::Router::new()
    .route(
      "/demands",
      post(|State(cards): State<Arc<Cards>>, axum::Json(ask): axum::Json<Value>| async move {
        let key = ask["workKey"].as_str().unwrap_or_default().to_owned();
        let id = {
          let mut asked = cards.asked.lock().unwrap();
          let id = 100 + asked.len() as i64;

          asked.push((id, key.clone()));
          id
        };

        (StatusCode::ACCEPTED, axum::Json(card_demand(&cards, id, &key)))
      })
      .get(|State(cards): State<Arc<Cards>>| async move {
        axum::Json(json!({ "items": held_demands(&cards), "more": false }))
      }),
    )
    .route(
      "/demands/{id}",
      get(|State(cards): State<Arc<Cards>>, Path(id): Path<i64>| async move {
        match held_demands(&cards).into_iter().find(|demand| demand["id"] == id) {
          Some(demand) => (StatusCode::OK, axum::Json(demand)),
          None => (StatusCode::NOT_FOUND, axum::Json(json!({ "code": "not-found", "error": "no demand" }))),
        }
      })
      .delete(|State(cards): State<Arc<Cards>>, Path(id): Path<i64>| async move {
        cards.given.lock().unwrap().push(id);

        StatusCode::NO_CONTENT
      }),
    )
    .with_state(cards);
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a port");
  let at = listener.local_addr().expect("its address");

  tokio::spawn(async move { axum::serve(listener, router).await });

  at
}

/// A stand-in node service that is asked for a node's target and never answers, so a line granted its card stays
/// `speaking` for as long as nothing ends it; whether it was asked is kept.
async fn silent_nodes(asked: Arc<std::sync::atomic::AtomicBool>) -> std::net::SocketAddr {
  use axum::extract::State;
  use axum::routing::get;

  let router = axum::Router::new()
    .route(
      "/nodes/{name}/target",
      get(|State(asked): State<Arc<std::sync::atomic::AtomicBool>>| async move {
        asked.store(true, Ordering::SeqCst);
        std::future::pending::<StatusCode>().await
      }),
    )
    .with_state(asked);
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a port");
  let at = listener.local_addr().expect("its address");

  tokio::spawn(async move { axum::serve(listener, router).await });

  at
}

#[tokio::test]
async fn switching_off_stops_a_waiting_and_a_speaking_briefing_and_neither_plays_afterwards() {
  let scratch = Scratch::new().await;
  let cards = Arc::new(Cards {
    granted: "voice:synthesis:1".to_owned(),
    asked: Default::default(),
    given: Default::default(),
  });
  let node_asked = Arc::new(std::sync::atomic::AtomicBool::new(false));
  let master = cards_master(Arc::clone(&cards)).await;
  let nodes = silent_nodes(Arc::clone(&node_asked)).await;
  let served = served(&scratch, Some(master), Some(nodes)).await;
  let engine = router(served.clone());
  let briefing = |room: i64| {
    json!({ "voiceName": "voice-b", "text": "The backup finished.", "spoken": SPOKEN, "chatId": room,
            "kind": "summary" })
  };
  let (_, speaking) = call(&engine, "POST", "/briefings", Some(briefing(ROOM))).await;
  let (_, waiting) = call(&engine, "POST", "/briefings", Some(briefing(9))).await;
  let ids = [speaking["id"].clone(), waiting["id"].clone()];

  assert_eq!(speaking["synthesisId"], 1, "the stand-in master grants the first synthesis its card");

  // The first is granted its card and speaks on a node that never answers; the second waits for its card.
  let mut states = (Value::Null, Value::Null);

  for _ in 0..400 {
    states = (
      call(&engine, "GET", "/syntheses/1", None).await.1,
      call(&engine, "GET", "/syntheses/2", None).await.1,
    );

    if node_asked.load(Ordering::SeqCst)
      && states.0["state"] == "speaking"
      && states.1["waitingOn"].is_string()
    {
      break;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  assert_eq!(states.0["state"], "speaking", "{}", states.0);
  assert!(node_asked.load(Ordering::SeqCst), "the speaking line reached its node");
  assert_eq!((states.1["state"].as_str(), &states.1["waitingOn"]), (Some("waiting"), &json!(IN_USE)));

  // Switching the briefings off stops every room's.
  let (status, stopped) = call(&engine, "POST", "/briefings/stop", Some(json!({}))).await;

  assert_eq!(status, StatusCode::OK, "{stopped}");

  let items = stopped["items"].as_array().expect("a page");

  assert_eq!(items.iter().map(|one| one["id"].clone()).collect::<Vec<_>>(), ids, "{stopped}");
  assert!(
    items.iter().all(|one| one["state"] == "stopped" && one["finishedAt"].is_i64()),
    "{stopped}"
  );

  // Both lines' speaking ends: each gives its card back, the speaking one while its node never answered.
  for _ in 0..400 {
    if cards.given.lock().unwrap().len() == 2 {
      break;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  let mut given = cards.given.lock().unwrap().clone();

  given.sort_unstable();
  assert_eq!(given, [100, 101], "both cards are given back");

  // Their speaking over, neither is done, neither serves a sound, and the only ends told are the stop's.
  for id in &ids {
    let (_, read) = call(&engine, "GET", &format!("/briefings/{id}"), None).await;
    let (status, _) = call(&engine, "GET", &format!("/briefings/{id}/sound"), None).await;

    assert_eq!((read["state"].as_str(), status), (Some("stopped"), StatusCode::NOT_FOUND), "{read}");
  }

  let (_, page) = call(&engine, "GET", "/events?kinds=briefing.ended,briefings.stopped", None).await;
  let told: Vec<(&str, &Value)> = page["items"]
    .as_array()
    .expect("a page")
    .iter()
    .map(|event| (event["kind"].as_str().unwrap_or_default(), &event["data"]))
    .collect();

  assert_eq!(told.len(), 3, "{page}");
  assert!(
    told[..2]
      .iter()
      .all(|(kind, data)| *kind == "briefing.ended" && data["state"] == "stopped"),
    "{page}"
  );
  assert_eq!(told[2], ("briefings.stopped", &json!({ "chatId": null, "briefingIds": ids })));

  // Switched on again, or started afresh, the engine takes up no line that was stopped.
  let taken = bmo_voice::http::take_up(Arc::clone(&served.context), served.events.clone())
    .await
    .expect("the unspoken lines are read");

  assert_eq!(taken, 0);
}

#[test]
fn a_reader_naming_no_cursor_starts_a_page_at_the_oldest_kept_and_a_stream_at_the_newest() {
  use bmo_voice::http::event_start;

  assert_eq!(event_start(Some(41), false, Some(10), Some(50)), 41, "a cursor named is the start");
  assert_eq!(event_start(Some(41), true, Some(10), Some(50)), 41);
  assert_eq!(
    event_start(None, true, Some(1200), Some(2199)),
    1199,
    "a page replays from the oldest kept"
  );
  assert_eq!(
    event_start(None, false, Some(1200), Some(2199)),
    2199,
    "a stream carries the live ones only"
  );
  assert_eq!(event_start(None, true, None, None), 0, "nothing kept yet");
  assert_eq!(event_start(None, false, None, None), 0);
}
