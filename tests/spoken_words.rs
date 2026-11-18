//! The spoken words (`docs/boundary-api.md`, 「The spoken words」, 「How the engine derives the spoken words」): a
//! line's spoken words given by its caller are held to the check and refused naming the first word that fails, never
//! mended; a line given its written words alone has them derived on a slot of agency's pool, the agreed readings
//! handed over first, an answer that fails asked again naming the word, and the readings the model chose kept
//! without writing over a row that stands; and with no agency named the line waits, saying so.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{Request, StatusCode, Uri};
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

use super::scratch::{Scratch, address_of};

const TOKEN: &str = "a-test-token";

/// The engine's routes on the scratch store with one served voice, its master out of reach so no line it takes in is
/// ever spoken, and its agency the one `agency` names, or none.
async fn engine(scratch: &Scratch, agency: Option<std::net::SocketAddr>) -> axum::Router {
  router(served(scratch, agency).await)
}

/// What [`engine`] serves: its context and its event log, for a test that runs the service's own start steps.
async fn served(scratch: &Scratch, agency: Option<std::net::SocketAddr>) -> Served {
  let master = "[::1]:9";
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
address = "http://[::1]:9"

[units.arbiter]
address = "http://{master}"

[units.web]
address = "http://[::1]:9"
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
    reads: Default::default(),
    clocks: Default::default(),
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

  // Spoken words past the most a line holds do not stand, and are asked again; at the bound they stand.
  let most = bmo_voice::speech::spoken::MOST_SPOKEN;
  let at_most = json!({ "spoken": "가".repeat(most) }).to_string();
  let past = json!({ "spoken": "가".repeat(most + 1) }).to_string();

  assert_eq!(testing::answer(&at_most, Language::Ko).map(|words| words.chars().count()), Ok(most));
  assert!(testing::answer(&past, Language::Ko).expect_err("too long").contains("30000 characters"));

  let agreed = [("Nuxt".to_owned(), "넉스트".to_owned())];
  let prompt = testing::prompt(Language::Ko, &agreed, "Nuxt and Qwen", None);

  assert!(prompt.contains("The line's language: Korean"), "{prompt}");
  assert!(prompt.contains("- Nuxt: 넉스트"), "{prompt}");
  assert!(prompt.contains("Nuxt and Qwen"), "{prompt}");
  assert!(
    prompt.contains("- 오점오: 오쩌모"),
    "a Korean turn shows runs written as they sound: {prompt}"
  );

  let english = testing::prompt(Language::En, &[], "Nuxt and Qwen", None);

  assert!(
    !english.contains("the way it sounds"),
    "the Korean examples stay out of an English turn: {english}"
  );
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
  slots: Mutex<Vec<Value>>,
  admissions: AtomicU32,
  bound: Mutex<Vec<Value>>,
  starts: Mutex<Vec<Value>>,
  prompts: Mutex<Vec<String>>,
  given_back: Mutex<Vec<Value>>,
  owners_released: Mutex<Vec<String>>,
  ended: Mutex<Vec<String>>,
  answers: Mutex<Vec<String>>,
}

// A refusal as agency answers one.
fn refused(status: StatusCode, code: &str, error: String) -> (StatusCode, axum::Json<Value>) {
  (status, axum::Json(json!({ "code": code, "error": error })))
}

// The slot the stand-in lets in: the role's order's road.
fn slot_of(state: &str) -> Value {
  json!({ "token": "slot-1", "state": state, "role": "transliterator", "path": null, "provider": "deepseek",
    "model": "deepseek-flash", "effort": "low", "heldFor": null })
}

async fn stand_in_agency(asked: Arc<Asked>) -> std::net::SocketAddr {
  let router = axum::Router::new()
    .route(
      "/slot-requests",
      post(|State(asked): State<Arc<Asked>>, axum::Json(ask): axum::Json<Value>| async move {
        // A slot's ask as agency reads it: a request key, an owner and a role, and a road named whole or not at all.
        if ["requestKey", "owner", "role"].iter().any(|part| !ask[*part].is_string()) {
          return refused(StatusCode::BAD_REQUEST, "bad-request", format!("an ask lacks a part: {ask}"));
        }

        if ask.get("provider").is_some() != ask.get("model").is_some() {
          let why = "A slot's road names its provider and its model together".to_owned();

          return refused(StatusCode::BAD_REQUEST, "bad-request", why);
        }

        asked.slots.lock().unwrap().push(ask);
        (StatusCode::CREATED, axum::Json(slot_of("waiting")))
      })
      // An owner's release: everything the owner holds given back.
      .delete(
        |State(asked): State<Arc<Asked>>,
         axum::extract::Query(query): axum::extract::Query<std::collections::BTreeMap<String, String>>| async move {
          let owner = query.get("owner").cloned().unwrap_or_default();
          let held = asked.slots.lock().unwrap().iter().filter(|ask| ask["owner"] == owner.as_str()).count();

          asked.owners_released.lock().unwrap().push(owner);
          axum::Json(json!({ "cancelled": held, "released": 0, "reservations": 0, "sessionsEnded": [] }))
        },
      ),
    )
    .route(
      "/slot-requests/{token}",
      patch(|State(asked): State<Arc<Asked>>, axum::Json(change): axum::Json<Value>| async move {
        match change["state"].as_str() {
          Some("finished") => {
            asked.given_back.lock().unwrap().push(change);
            axum::Json(json!({ "held": true, "stoodDown": false, "note": null }))
          }
          Some(_) => {
            asked.bound.lock().unwrap().push(change);
            axum::Json(slot_of("active"))
          }
          // Its admission: its turn comes on the owner's second ask.
          None => {
            let (request, state) = match asked.admissions.fetch_add(1, Ordering::SeqCst) {
              0 => (Value::Null, "waiting"),
              _ => (slot_of("active"), "active"),
            };

            axum::Json(
              json!({ "request": request, "state": state, "endedBy": null, "outcome": null, "demand": null }),
            )
          }
        }
      }),
    )
    .route(
      "/sessions",
      post(|State(asked): State<Arc<Asked>>, axum::Json(start): axum::Json<Value>| async move {
        // A role's start as agency reads it: on an active slot, naming no road of its own that is not the slot's.
        if start["scope"] != "role" {
          return refused(StatusCode::BAD_REQUEST, "bad-request", format!("not a role's start: {start}"));
        }

        if start["slot"] != "slot-1" || asked.admissions.load(Ordering::SeqCst) < 2 {
          let why = format!("No active slot request holds the token {}", start["slot"]);

          return refused(StatusCode::UNPROCESSABLE_ENTITY, "slot-not-active", why);
        }

        let slot = slot_of("active");

        if let Some(field) = ["role", "provider", "model"]
          .into_iter()
          .find(|field| start.get(*field).is_some_and(|named| *named != slot[*field]))
        {
          let why = format!("The slot's {field} is not {}", start[field]);

          return refused(StatusCode::UNPROCESSABLE_ENTITY, "contradicts-slot", why);
        }

        asked.starts.lock().unwrap().push(start);
        (StatusCode::CREATED, axum::Json(json!({ "id": "transliterator@run:1" })))
      }),
    )
    .route(
      "/sessions/{session}/turns",
      post(
        |State(asked): State<Arc<Asked>>, uri: Uri, axum::Json(turn): axum::Json<Value>| async move {
          // The session's id as agency reads it: its segment unescaped, so an escaped `@` or `:` names no session.
          if uri.path() != "/sessions/transliterator@run:1/turns" {
            return refused(
              StatusCode::NOT_FOUND,
              "unknown-session",
              format!("no session at {}", uri.path()),
            );
          }

          // A role's session's turn names no road: its provider, model and effort are the session's.
          if let Some(part) = ["provider", "model", "mapping", "workKey"]
            .iter()
            .find(|part| turn.get(**part).is_some())
          {
            return refused(StatusCode::BAD_REQUEST, "bad-request", format!("a turn names no `{part}`"));
          }

          if !turn["prompt"].is_string() {
            return refused(StatusCode::BAD_REQUEST, "bad-request", "missing field `prompt`".to_owned());
          }

          asked
            .prompts
            .lock()
            .unwrap()
            .push(turn["prompt"].as_str().unwrap_or_default().to_owned());
          let answer = asked.answers.lock().unwrap().remove(0);
          (StatusCode::CREATED, axum::Json(json!({ "answer": answer })))
        },
      ),
    )
    .route(
      "/sessions/{session}",
      delete(|State(asked): State<Arc<Asked>>, Path(session): Path<String>| async move {
        asked.ended.lock().unwrap().push(session);
        StatusCode::NO_CONTENT
      }),
    )
    .with_state(asked);
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a port");
  let at = listener.local_addr().expect("its address");

  tokio::spawn(async move { axum::serve(listener, router).await });

  at
}

#[tokio::test]
async fn written_words_alone_are_derived_on_a_slot_the_agreed_readings_first_and_a_failed_answer_asked_again()
 {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());

  *asked.answers.lock().unwrap() = vec![
    r#"{"spoken": "넉스트와 Qwen 모델", "readings": []}"#.to_owned(),
    r#"{"spoken": "넉스트와 퀜 모델", "readings": [{"written": "Qwen", "said": "퀜"}, {"written": "Nuxt", "said": "눅스트"}, {"written": "7dae3520e72b", "said": "칠디에이이"}]}"#
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

  let slot = asked.slots.lock().unwrap()[0].clone();

  assert_eq!(slot["role"], "transliterator");
  assert_eq!(slot["source"], "voice");
  assert_eq!(slot["owner"], format!("voice:synthesis:{id}"), "a slot's owner is its synthesis");
  assert!(
    slot["requestKey"]
      .as_str()
      .and_then(|key| key.strip_prefix(&format!("voice:synthesis:{id}:")))
      .is_some_and(|drawn| drawn.len() == 16 && drawn.chars().all(|c| c.is_ascii_hexdigit())),
    "its request key is its owner and a number drawn for the ask: {slot}"
  );
  assert!(
    slot.get("provider").is_none() && slot.get("model").is_none(),
    "the role's order picks the road: {slot}"
  );
  assert!(
    asked.admissions.load(Ordering::SeqCst) >= 2,
    "a waiting slot's admission is asked again"
  );
  assert!(
    asked.bound.lock().unwrap().is_empty(),
    "the engine binds no slot: agency binds it at the start"
  );

  let start = asked.starts.lock().unwrap()[0].clone();

  assert_eq!(start["scope"], "role");
  assert_eq!(start["slot"], "slot-1");
  assert_eq!(start["line"], "search");
  assert_eq!(start["toolsAllowed"], json!([]));
  assert!(start["systemPrompt"].as_str().is_some_and(|prompt| prompt.contains("spoken words")));
  assert_eq!(asked.slots.lock().unwrap().len(), 1, "one slot stands for the whole derivation");
  assert_eq!(asked.starts.lock().unwrap().len(), 1, "and one session: every try is a turn in it");
  assert_eq!(asked.given_back.lock().unwrap().len(), 1, "the slot is given back once the words stand");
  assert_eq!(*asked.ended.lock().unwrap(), ["transliterator@run:1"], "and the session ended");

  let reading = |written: &str| format!(r#"SELECT "ko" FROM "voice_names" WHERE "written" = '{written}'"#);
  let qwen: String = store
    .fetch_one(reading("Qwen").as_str())
    .await
    .expect("the chosen reading is kept")
    .get("ko");
  let nuxt: String = store.fetch_one(reading("Nuxt").as_str()).await.expect("the row stands").get("ko");

  assert_eq!(qwen, "퀜");
  assert_eq!(nuxt, "넉스트", "a row that stands is never written over");

  let hashes: i64 = store
    .fetch_one(r#"SELECT count(*) AS "rows" FROM "voice_names" WHERE "written" = '7dae3520e72b'"#)
    .await
    .expect("the table is read")
    .get("rows");

  assert_eq!(hashes, 0, "an identifier is no name and gets no row");

  store.close().await.expect("the store connection closes");
}

#[tokio::test]
async fn the_settled_month_and_day_readings_are_handed_over_and_the_line_kept_whole() {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());
  let line = "오늘 아침 브리핑입니다. 시장은 어제보다 조금 안정을 되찾았고, 투자자들은 다음 주에 나올 물가 \
    지표와 금리 전망을 기다리고 있습니다. 11월 5일 발표된 지표 가운데 소비자 물가가 가장 눈에 띄었고, \
    시장의 반응도 잠잠했습니다.";
  let heard = line.replace("11월 5일", "십일월 오일");

  *asked.answers.lock().unwrap() = vec![json!({ "spoken": heard, "readings": [] }).to_string()];

  let engine = engine(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await)).await;
  let mut store = PgConnection::connect(&address_of(&scratch.name)).await.expect("the store answers");

  store
    .execute(
      r#"INSERT INTO "voice_names" ("written", "ko", "en", "set_at")
         VALUES ('11월', '십일월', '', 0), ('5일', '오일', '', 0)"#,
    )
    .await
    .expect("the settled readings are written");

  let (status, made) =
    call(&engine, "POST", "/syntheses", Some(json!({ "voiceName": "voice-a", "text": line }))).await;

  assert_eq!(status, StatusCode::CREATED, "{made}");

  let id = made["id"].as_i64().expect("an id");
  let read = read_until(&engine, id, |read| read["spoken"].is_string()).await;

  assert_eq!(read["text"], line, "the written line stands as given, nothing of it lost");
  assert_eq!(read["spoken"], heard, "{read}");

  let prompt = asked.prompts.lock().unwrap()[0].clone();

  assert!(
    prompt.contains("- 11월: 십일월"),
    "the month's settled reading is handed over: {prompt}"
  );
  assert!(prompt.contains("- 5일: 오일"), "the day's settled reading is handed over: {prompt}");
  assert!(prompt.contains(line), "the written line reaches the model whole: {prompt}");

  store.close().await.expect("the store connection closes");
}

#[tokio::test]
async fn what_the_syntheses_an_earlier_process_left_hold_is_given_back_by_their_owners() {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());
  let served = served(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await)).await;
  let mut store = PgConnection::connect(&address_of(&scratch.name)).await.expect("the store answers");

  store
    .execute(
      r#"INSERT INTO "voice_syntheses" ("id", "voice_name", "text", "spoken", "language", "state", "created_at")
         VALUES (910, 'voice-a', 'Left.', NULL, 'ko', 'waiting', 0),
                (911, 'voice-a', 'Done.', '끝.', 'ko', 'done', 0)"#,
    )
    .await
    .expect("the lines a process left are written");
  asked.slots.lock().unwrap().push(json!({ "owner": "voice:synthesis:910" }));

  let agency = served.context.units.agency.as_ref().expect("an agency is named");
  let given = bmo_voice::speech::spoken::give_back_left(agency, &served.context.db).await;

  assert_eq!(given, 1, "the slot the left synthesis asked is given back");
  assert_eq!(
    *asked.owners_released.lock().unwrap(),
    ["voice:synthesis:910"],
    "only an unfinished synthesis's owner is asked to give back what it holds"
  );
  assert!(asked.given_back.lock().unwrap().is_empty(), "no slot is given back by its token");

  store.close().await.expect("the store connection closes");
}

#[tokio::test]
async fn a_spoken_road_names_its_provider_and_its_model_together() {
  let scratch = Scratch::new().await;
  let base = format!(
    r#"listen = "127.0.0.1:0"
address = "http://127.0.0.1:0"
token_file = "voice.token"
database = "{}"
data_root = "{}"

[units.node]
address = "http://[::1]:9"

[units.arbiter]
address = "http://[::1]:9"

[units.web]
address = "http://[::1]:9"

[spoken]
"#,
    address_of(&scratch.name),
    scratch.root.display()
  );
  let read = |name: &str, road: &str| {
    let path = scratch.root.join("env").join(name);

    std::fs::write(&path, format!("{base}{road}")).expect("the settings are written");
    Settings::read(&path)
  };

  let alone = read("alone.toml", "provider = \"deepseek\"\n");

  assert!(
    alone.as_ref().is_err_and(|why| why.0.contains("provider and model together")),
    "a provider alone does not stand: {alone:?}"
  );

  let whole = read("whole.toml", "provider = \"deepseek\"\nmodel = \"deepseek-flash\"\n")
    .expect("a whole road stands");

  assert_eq!(whole.spoken.provider.as_deref(), Some("deepseek"));
  assert_eq!(whole.spoken.model.as_deref(), Some("deepseek-flash"));

  let neither = read("neither.toml", "").expect("no road stands, the role's order picking");

  assert_eq!(neither.spoken.role, "transliterator");
}

// A line a stopped process left goes on: its derivation or its card waits however long it takes, so the runtime is let
// go without waiting for what was taken up.
#[test]
fn the_lines_a_stopped_process_left_are_taken_up_as_the_next_starts() {
  let runtime = tokio::runtime::Runtime::new().expect("a runtime");

  runtime.block_on(takes_up_left_lines());
  runtime.shutdown_background();
}

async fn takes_up_left_lines() {
  let scratch = Scratch::new().await;
  let served = served(&scratch, None).await;
  let engine = router(Served {
    context: Arc::clone(&served.context),
    events: served.events.clone(),
    token: served.token.clone(),
  });
  let mut store = PgConnection::connect(&address_of(&scratch.name)).await.expect("the store answers");

  store
    .execute(
      r#"INSERT INTO "voice_syntheses" ("id", "voice_name", "text", "spoken", "language", "state", "created_at")
         VALUES (900, 'voice-a', 'Qwen is up.', '퀜이 섰습니다.', 'ko', 'speaking', 0),
                (901, 'voice-gone', 'Gone.', NULL, NULL, 'waiting', 0),
                (902, 'voice-a', 'Qwen is up.', NULL, 'ko', 'waiting', 0)"#,
    )
    .await
    .expect("the lines a process left are written");

  let taken = bmo_voice::http::take_up(Arc::clone(&served.context), served.events.clone())
    .await
    .expect("the left lines are read");

  assert_eq!(taken, 2, "the lines whose voice stands are taken up");

  let spoken = read_until(&engine, 900, |read| read["state"] == "failed").await;

  assert_eq!(spoken["spoken"], "퀜이 섰습니다.", "it is spoken from its own spoken words: {spoken}");

  let (_, gone) = call(&engine, "GET", "/syntheses/901", None).await;

  assert_eq!(gone["state"], "failed", "a line whose voice is gone fails: {gone}");

  let deriving = read_until(&engine, 902, |read| {
    read["waitingOn"].as_str().is_some_and(|on| on.contains("no agency is named"))
  })
  .await;

  assert_eq!(deriving["state"], "waiting", "a line with no spoken words has them derived: {deriving}");

  store.close().await.expect("the store connection closes");
}
