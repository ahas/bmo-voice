//! The spoken words (`docs/boundary-api.md`, 「The spoken words」, 「How the engine derives the spoken words」): a
//! line's spoken words given by its caller are held to the check and refused naming the first word that fails, never
//! mended; a line given its written words alone has them derived on a seat of agency's pool, the agreed readings
//! handed over first, an answer that fails asked again naming the word, and the readings the model chose kept
//! without writing over a row that stands; and with no agency named the line waits, saying so.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Request, StatusCode};
use axum::routing::{delete, patch, post};
use bmo_voice::context::Context;
use bmo_voice::events::Events;
use bmo_voice::http::{Served, router};
use bmo_voice::script::reading::Language;
use bmo_voice::script::unconverted::unconverted;
use bmo_voice::settings::Settings;
use bmo_voice::speech::spoken::testing;
use bmo_voice::units::Units;
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor, Row};
use tower::ServiceExt;

static MADE: AtomicU32 = AtomicU32::new(0);

const TOKEN: &str = "a-test-token";

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
    let name = format!("bmo_voice_spoken_test_{}_{made}", std::process::id());
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

/// The engine's routes on the scratch store with one served voice, its master out of reach so no line it takes in is
/// ever spoken, and its agency the one `agency` names, or none.
async fn engine(scratch: &Scratch, agency: Option<std::net::SocketAddr>) -> axum::Router {
  let master = "127.0.0.1:9";
  let agency =
    agency.map_or_else(String::new, |at| format!("\n[units.agency]\naddress = \"http://{at}\"\n"));
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
{agency}"#,
    address_of(&scratch.name),
    scratch.root.display()
  );

  std::fs::write(&settings_path, settings).expect("the settings are written");

  let settings = Settings::read(&settings_path).expect("the settings read");
  let db = bmo_voice::store::connect(&settings.database, 4).await.expect("the store opens");

  bmo_voice::store::migrate(&db).await.expect("the store migrates");
  // The one voice, served, its folder outside the voices root, so a line in it is never spoken.
  let folder = scratch.root.join("elsewhere").join("voice-a");

  std::fs::create_dir_all(&folder).expect("the voice folder is made");
  std::fs::write(folder.join("voice.toml"), "engine = \"fish-s2-pro\"\n").expect("the voice is declared");
  db.execute_unprepared(&format!(
    r#"INSERT INTO "voices" ("name", "checkpoint", "reference", "cfg", "steps", "set_at")
       VALUES ('voice-a', '{}', '', 0, 0, 0)"#,
    folder.display()
  ))
  .await
  .expect("the voice is written");

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

async fn read_until(engine: &axum::Router, id: i64, done: impl Fn(&Value) -> bool) -> Value {
  let mut read = Value::Null;

  for _ in 0..400 {
    (_, read) = call(engine, "GET", &format!("/syntheses/{id}"), None).await;

    if done(&read) {
      return read;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  panic!("synthesis {id} never came to it: {read}");
}

#[test]
fn the_check_finds_the_first_word_left_unconverted_and_only_finds_it() {
  assert_eq!(unconverted("오늘 백업이 끝났습니다.", Language::Ko), None);

  let found = unconverted("오늘 큐웬 Qwen3 모델이 섰습니다", Language::Ko).expect("a Latin word");

  assert_eq!(found.word, "Qwen3");
  assert_eq!(found.holds, "a Latin letter");
  assert_eq!(unconverted("이천이십육 년 3월", Language::Ko).expect("a digit").holds, "a digit");
  assert_eq!(unconverted("문서는 docs/layout 에", Language::Ko).expect("a path").word, "docs/layout");
  assert!(unconverted("강조된 **말**", Language::Ko).is_some(), "markup");
  assert!(unconverted("   ", Language::Ko).is_some(), "nothing to say");
  assert!(unconverted("a mark \u{E000} here", Language::En).is_some(), "a private mark");
  assert_eq!(unconverted("The backup finished.", Language::En), None);
  assert_eq!(
    unconverted("The 백업 finished.", Language::En).expect("Hangul").holds,
    "a Hangul letter"
  );
}

#[test]
fn an_answer_is_read_out_of_the_text_round_it_and_held_to_the_check() {
  let fenced = "```json\n{\"spoken\": \"백업이 끝났습니다.\", \"readings\": []}\n```";

  assert_eq!(testing::answer(fenced, Language::Ko).as_deref(), Ok("백업이 끝났습니다."));

  let latin = testing::answer("{\"spoken\": \"Qwen 모델\"}", Language::Ko).expect_err("a Latin word");

  assert!(latin.contains("\"Qwen\""), "{latin}");
  assert!(testing::answer("no object here", Language::Ko).is_err());

  let agreed = [("Nuxt".to_owned(), "넉스트".to_owned())];
  let prompt = testing::prompt(Language::Ko, &agreed, "Nuxt and Qwen", None);

  assert!(prompt.contains("The line's language: Korean"), "{prompt}");
  assert!(prompt.contains("- Nuxt: 넉스트"), "{prompt}");
  assert!(prompt.contains("Nuxt and Qwen"), "{prompt}");
}

#[tokio::test]
async fn spoken_words_a_caller_gives_are_checked_and_refused_naming_the_word_never_mended() {
  let scratch = Scratch::new().await;
  let engine = engine(&scratch, None).await;
  let (status, refused) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-a", "text": "Qwen is up.", "spoken": "Qwen 이 섰습니다." })),
  )
  .await;

  assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
  assert_eq!(refused["code"], "unconverted-run");
  assert!(refused["error"].as_str().unwrap_or_default().contains("\"Qwen\""), "{refused}");

  let (status, refused) = call(
    &engine,
    "POST",
    "/briefings",
    Some(
      json!({ "voiceName": "voice-a", "text": "Up at 3.", "spoken": "3시에 섰습니다.", "chatId": 1,
      "kind": "summary" }),
    ),
  )
  .await;

  assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
  assert_eq!(refused["code"], "unconverted-run");

  let (status, made) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-a", "text": "Qwen is up.", "spoken": "퀜이 섰습니다." })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{made}");
  assert_eq!(made["text"], "Qwen is up.", "the written words are kept as given");
  assert_eq!(made["spoken"], "퀜이 섰습니다.");

  let (status, made) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(
      json!({ "voiceName": "voice-a", "text": "Qwen is up.", "spoken": "Kwen is up.", "language": "en" }),
    ),
  )
  .await;

  assert_eq!(
    status,
    StatusCode::CREATED,
    "an English line in a Korean voice is checked in English: {made}"
  );

  let (status, _) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-a", "text": "Up.", "spoken": "Up.", "language": "fr" })),
  )
  .await;

  assert_eq!(status, StatusCode::BAD_REQUEST);
}

// The derivation of a line with no agency named never ends, so the runtime is let go without waiting for it.
#[test]
fn written_words_alone_wait_saying_so_while_no_agency_is_named() {
  let runtime = tokio::runtime::Runtime::new().expect("a runtime");

  runtime.block_on(waits_with_no_agency());
  runtime.shutdown_background();
}

async fn waits_with_no_agency() {
  let scratch = Scratch::new().await;
  let engine = engine(&scratch, None).await;
  let (status, made) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-a", "text": "Qwen is up." })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{made}");
  assert_eq!(made["spoken"], Value::Null);

  let id = made["id"].as_i64().expect("an id");
  let read = read_until(&engine, id, |read| {
    read["waitingOn"].as_str().is_some_and(|on| on.contains("no agency is named"))
  })
  .await;

  assert_eq!(read["state"], "waiting", "{read}");
  assert_eq!(read["spoken"], Value::Null);
}

/// What the stand-in agency was asked, and the answers its turns give in order.
#[derive(Default)]
struct Asked {
  seats: Mutex<Vec<Value>>,
  starts: Mutex<Vec<Value>>,
  prompts: Mutex<Vec<String>>,
  given_back: Mutex<Vec<Value>>,
  answers: Mutex<Vec<String>>,
}

async fn stand_in_agency(asked: Arc<Asked>) -> std::net::SocketAddr {
  let router = axum::Router::new()
    .route(
      "/seat-requests",
      post(|State(asked): State<Arc<Asked>>, axum::Json(ask): axum::Json<Value>| async move {
        asked.seats.lock().unwrap().push(ask);
        axum::Json(json!({ "token": "seat-1", "state": "active", "provider": "deepseek",
          "model": "deepseek-flash", "effort": "low", "tier": "Light" }))
      }),
    )
    .route(
      "/seat-requests/{token}",
      patch(|State(asked): State<Arc<Asked>>, axum::Json(change): axum::Json<Value>| async move {
        if change["state"] == "finished" {
          asked.given_back.lock().unwrap().push(change);
        }
        axum::Json(json!({ "held": true, "stoodDown": false, "note": null }))
      }),
    )
    .route(
      "/sessions",
      post(|State(asked): State<Arc<Asked>>, axum::Json(start): axum::Json<Value>| async move {
        asked.starts.lock().unwrap().push(start);
        (StatusCode::CREATED, axum::Json(json!({ "id": "apart-1" })))
      }),
    )
    .route(
      "/sessions/{session}/turns",
      post(|State(asked): State<Arc<Asked>>, axum::Json(turn): axum::Json<Value>| async move {
        asked
          .prompts
          .lock()
          .unwrap()
          .push(turn["prompt"].as_str().unwrap_or_default().to_owned());
        let answer = asked.answers.lock().unwrap().remove(0);
        (StatusCode::CREATED, axum::Json(json!({ "answer": answer })))
      }),
    )
    .route("/sessions/{session}", delete(|| async { StatusCode::NO_CONTENT }))
    .with_state(asked);
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a port");
  let at = listener.local_addr().expect("its address");

  tokio::spawn(async move { axum::serve(listener, router).await });

  at
}

#[tokio::test]
async fn written_words_alone_are_derived_on_a_seat_the_agreed_readings_first_and_a_failed_answer_asked_again()
 {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());

  *asked.answers.lock().unwrap() = vec![
    r#"{"spoken": "넉스트와 Qwen 모델", "readings": []}"#.to_owned(),
    r#"{"spoken": "넉스트와 퀜 모델", "readings": [{"written": "Qwen", "said": "퀜"}, {"written": "Nuxt", "said": "눅스트"}]}"#
      .to_owned(),
  ];

  let engine = engine(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await)).await;
  let mut store = PgConnection::connect(&address_of(&scratch.name)).await.expect("the store answers");

  store
    .execute(
      r#"INSERT INTO "voice_names" ("written", "ko", "en", "set_at") VALUES ('Nuxt', '넉스트', '', 0)"#,
    )
    .await
    .expect("an agreed reading is written");

  let (status, made) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-a", "text": "Nuxt and the Qwen model" })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{made}");

  let id = made["id"].as_i64().expect("an id");
  let read = read_until(&engine, id, |read| read["spoken"].is_string()).await;

  assert_eq!(read["spoken"], "넉스트와 퀜 모델", "{read}");
  assert_eq!(read["text"], "Nuxt and the Qwen model", "the written words stand as given");

  let prompts = asked.prompts.lock().unwrap().clone();

  assert_eq!(prompts.len(), 2, "the failed answer is asked again: {prompts:?}");
  assert!(prompts[0].contains("- Nuxt: 넉스트"), "the agreed readings come first: {}", prompts[0]);
  assert!(prompts[0].contains("Nuxt and the Qwen model"), "{}", prompts[0]);
  assert!(prompts[1].contains("\"Qwen\"") && prompts[1].contains("Latin letter"), "{}", prompts[1]);

  let seat = asked.seats.lock().unwrap()[0].clone();

  assert_eq!(seat["role"], "transliterator");
  assert_eq!(seat["workKey"], "role:transliterator");

  let start = asked.starts.lock().unwrap()[0].clone();

  assert_eq!(start["scope"], "apart");
  assert_eq!(start["line"], "search");
  assert_eq!(start["toolsAllowed"], json!([]));
  assert_eq!(start["model"], "deepseek-flash");
  assert!(start["systemPrompt"].as_str().is_some_and(|prompt| prompt.contains("spoken words")));
  assert_eq!(asked.given_back.lock().unwrap().len(), 2, "every seat is given back");

  let reading = |written: &str| format!(r#"SELECT "ko" FROM "voice_names" WHERE "written" = '{written}'"#);
  let qwen: String = store
    .fetch_one(reading("Qwen").as_str())
    .await
    .expect("the chosen reading is kept")
    .get("ko");
  let nuxt: String = store.fetch_one(reading("Nuxt").as_str()).await.expect("the row stands").get("ko");

  assert_eq!(qwen, "퀜");
  assert_eq!(nuxt, "넉스트", "a row that stands is never written over");

  store.close().await.expect("the store connection closes");
}
