//! The spoken words (`docs/boundary-api.md`, 「The spoken words」, 「How the engine derives the spoken words」): a
//! line's spoken words given by its caller are held to the check and refused naming the first word that fails, never
//! mended; a line given its written words alone has them derived on a slot of agency's pool asked for the asker the
//! line names, a briefing's, a synthesis's and a line's taken up after a restart alike, the agreed readings
//! handed over first, an answer that fails asked again naming the word, and the readings the model chose kept
//! without writing over a row that stands; with no agency named the line waits, saying so; a slot the pool holds
//! back says what it is held for while the line waits, and one never let in ends the line naming that, its one
//! slot to a try and each of them given back; every try goes out under a name of its own, and a line whose tries
//! are spent fails saying why, its briefing carrying that why as its own; and a line stopped while its words are
//! derived gives back what it holds of agency.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{Request, StatusCode, Uri};
use axum::routing::{delete, get, post};
use bmo_voice::context::{Clocks, Context};
use bmo_voice::events::Events;
use bmo_voice::http::{Served, router};
use bmo_voice::script::reading::Language;
use bmo_voice::script::unconverted::unconverted;
use bmo_voice::settings::Settings;
use bmo_voice::speech::spoken::{Pacing, testing};
use bmo_voice::units::Units;
use serde_json::{Value, json};
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor, Row};
use tower::ServiceExt;

use super::scratch::{Scratch, address_of};

const TOKEN: &str = "a-test-token";

/// A derivation's tries paced in milliseconds, three to a line and the pool asked again every two, so no test waits
/// on the policy's seconds.
const QUICK: Pacing = Pacing {
  first_wait: Duration::from_millis(40),
  longest_wait: Duration::from_millis(80),
  tries: 3,
  ask_again: Duration::from_millis(2),
};

/// The policy's own pacing, whose first wait outlasts what a test reads.
fn policy_pacing() -> Pacing {
  Clocks::default().spoken
}

/// The engine's routes on the scratch store with one served voice, its master out of reach so no line it takes in is
/// ever spoken, its agency the one `agency` names, or none, and its derivations paced [`QUICK`].
async fn engine(scratch: &Scratch, agency: Option<std::net::SocketAddr>) -> axum::Router {
  router(served(scratch, agency, QUICK).await)
}

/// What [`engine`] serves, its derivations paced as `pacing` says: its context and its event log, for a test that runs
/// the service's own start steps.
async fn served(scratch: &Scratch, agency: Option<std::net::SocketAddr>, pacing: Pacing) -> Served {
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
    clocks: Clocks {
      spoken: pacing,
      ..Clocks::default()
    },
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
  read_within(engine, id, 400, done).await
}

// A synthesis read every 25 ms, at most `looks` times, until it is as `done` says.
async fn read_within(engine: &axum::Router, id: i64, looks: u32, done: impl Fn(&Value) -> bool) -> Value {
  let mut read = Value::Null;

  for _ in 0..looks {
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
fn a_lines_language_is_the_one_more_of_its_words_are_written_in() {
  assert_eq!(
    Language::of_line("BMO 왔어"),
    Some(Language::Ko),
    "as many Korean words as English is Korean"
  );
  assert_eq!(
    Language::of_line("`cargo check --all-targets`가 통과했어요"),
    Some(Language::Ko),
    "a word with a particle written onto it is a Korean word"
  );
  assert_eq!(
    Language::of_line("Say hello to 비모 for me, please."),
    Some(Language::En),
    "an English sentence carrying a Korean name stays English"
  );
  assert_eq!(
    Language::of_line("Ｈｅｌｌｏ ｔｈｅｒｅ"),
    Some(Language::En),
    "full-width Latin letters are Latin"
  );
  assert_eq!(Language::of_line("12:30 — 3.5%"), None, "figures alone say no language");
  assert_eq!(Language::of_line("   "), None);
}

#[tokio::test]
async fn a_briefing_naming_no_language_is_said_in_the_one_its_words_are_in() {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());
  let engine = engine(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await)).await;
  let lines = [
    (
      "Hi Haas! It's me, BMO! Now I am speaking English, just for you.",
      "Hi Haas! It's me, Beemo! Now I am speaking English, just for you.",
      "English",
    ),
    ("오늘 백업이 끝났습니다.", "오늘 백업이 끝났습니다.", "Korean"),
    (
      "오늘 receiver와 agency에 PR 두 개를 올렸어요.",
      "오늘 리시버와 에이전시에 피알 두 개를 올렸어요.",
      "Korean",
    ),
  ];

  for (at, (written, said, language)) in lines.into_iter().enumerate() {
    *asked.answers.lock().unwrap() = vec![json!({ "spoken": said, "readings": [] }).to_string()];

    let (status, made) = call(
      &engine,
      "POST",
      "/briefings",
      Some(
        json!({ "voiceName": "voice-a", "text": written, "chatId": 7, "kind": "speech", "plays": false }),
      ),
    )
    .await;

    assert_eq!(status, StatusCode::CREATED, "{made}");

    let synthesis = made["synthesisId"].as_i64().expect("its synthesis");
    let read = read_until(&engine, synthesis, |read| read["spoken"].is_string()).await;

    assert_eq!(read["spoken"], said, "{read}");
    assert!(
      asked.prompts.lock().unwrap()[at].contains(&format!("The line's language: {language}.")),
      "{written} is said in {language}"
    );
  }
}

#[test]
fn an_answer_is_read_out_of_the_text_round_it_and_held_to_the_check() {
  let fenced = "```json\n{\"spoken\": \"백업이 끝났습니다.\", \"readings\": []}\n```";

  assert_eq!(
    testing::answer(fenced, "The backup finished.", Language::Ko).as_deref(),
    Ok("백업이 끝났습니다.")
  );

  let latin =
    testing::answer("{\"spoken\": \"Qwen 모델\"}", "Qwen", Language::Ko).expect_err("a Latin word");

  assert!(latin.contains("\"Qwen\""), "{latin}");
  assert!(testing::answer("no object here", "", Language::Ko).is_err());

  // Spoken words past the most a line holds do not stand, and are asked again; at the bound they stand.
  let most = bmo_voice::speech::spoken::MOST_SPOKEN;
  let at_most = json!({ "spoken": "가".repeat(most) }).to_string();
  let past = json!({ "spoken": "가".repeat(most + 1) }).to_string();

  assert_eq!(testing::answer(&at_most, "", Language::Ko).map(|words| words.chars().count()), Ok(most));
  assert!(
    testing::answer(&past, "", Language::Ko)
      .expect_err("too long")
      .contains("30000 characters")
  );

  let misread = json!({ "spoken": "시피유 시비퍼센트 사용 중입니다." }).to_string();
  let refused = testing::answer(&misread, "CPU 12% 사용 중입니다.", Language::Ko).expect_err("12 misread");

  assert!(refused.contains("\"12%\""), "the answer is asked again naming the number: {refused}");

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
    Some(json!({ "voiceName": "voice-a", "text": "Qwen is up.", "spoken": "Qwen 이 섰습니다.", "language": "ko" })),
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
      json!({ "voiceName": "voice-a", "text": "Up at 3.", "spoken": "3시에 섰습니다.", "language": "ko", "chatId": 1,
      "kind": "summary" }),
    ),
  )
  .await;

  assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
  assert_eq!(refused["code"], "unconverted-run");

  let (status, refused) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(
      json!({ "voiceName": "voice-a", "text": "CPU 12%", "spoken": "시피유 시비퍼센트", "language": "ko" }),
    ),
  )
  .await;

  assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "a number not said as written: {refused}");
  assert!(refused["error"].as_str().unwrap_or_default().contains("\"12%\""), "{refused}");

  let (status, made) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-a", "text": "Qwen is up.", "spoken": "퀜이 섰습니다.", "language": "ko" })),
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

// The derivation of a line with no agency named waits the policy's seconds before its next try, so the runtime is let
// go without waiting for it.
#[test]
fn written_words_alone_wait_saying_so_while_no_agency_is_named() {
  let runtime = tokio::runtime::Runtime::new().expect("a runtime");

  runtime.block_on(waits_with_no_agency());
  runtime.shutdown_background();
}

async fn waits_with_no_agency() {
  let scratch = Scratch::new().await;
  let engine = router(served(&scratch, None, policy_pacing()).await);
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
  names: Mutex<Vec<String>>,
  given_back: Mutex<Vec<Value>>,
  owners_released: Mutex<Vec<String>>,
  ended: Mutex<Vec<String>>,
  answers: Mutex<Vec<String>>,
  /// Where the pool is to hold every slot back, the sentence it says why; the slot is never let in while it stands.
  hold: Mutex<Option<String>>,
}

// What marks a stand-in turn's answer as a turn that failed with no answer, its reason after it.
const ENDED: &str = "ended:";

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
      // The request as the pool holds it: its state and what it is held back for, which no admission answers.
      get(|State(asked): State<Arc<Asked>>| async move {
        let held = asked.hold.lock().unwrap().clone();
        let state = if held.is_some() { "waiting" } else { "active" };

        axum::Json(
          json!({ "token": "slot-1", "role": "transliterator", "state": state, "heldFor": held,
            "endedBy": null, "outcome": null }),
        )
      })
      .patch(|State(asked): State<Arc<Asked>>, axum::Json(change): axum::Json<Value>| async move {
        match change["state"].as_str() {
          Some("finished") => {
            asked.given_back.lock().unwrap().push(change);
            axum::Json(json!({ "held": true, "stoodDown": false, "note": null }))
          }
          Some(_) => {
            asked.bound.lock().unwrap().push(change);
            axum::Json(slot_of("active"))
          }
          // Its admission: its turn comes on the owner's second ask, unless the pool is held to answer nothing.
          None => {
            if asked.hold.lock().unwrap().is_some() {
              return axum::Json(
                json!({ "request": Value::Null, "state": "waiting", "endedBy": null, "outcome": null, "demand": null }),
              );
            }

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
          asked.names.lock().unwrap().push(turn["name"].as_str().unwrap_or_default().to_owned());
          let answer = asked.answers.lock().unwrap().remove(0);

          // An answer the test marks as an end is a turn that wrote nothing: agency answers its outcome and reason.
          match answer.strip_prefix(ENDED) {
            Some(reason) => (
              StatusCode::CREATED,
              axum::Json(json!({ "answer": null, "text": [], "outcome": "failed", "reason": reason })),
            ),
            None => (StatusCode::CREATED, axum::Json(json!({ "answer": answer }))),
          }
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
    r#"{"spoken": "넉스트와 퀜 모델", "readings": [{"written": "Qwen", "said": "퀜"}, {"written": "Nuxt", "said": "눅스트"}, {"written": "7dae3520e72b", "said": "칠디에이이"}, {"written": "11월", "said": "십일월"}, {"written": "12일", "said": "시비일"}]}"#
      .to_owned(),
  ];

  let engine = engine(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await)).await;
  let mut store = PgConnection::connect(&address_of(&scratch.name)).await.expect("the store answers");

  store
    .execute(
      r#"INSERT INTO "voice_names" ("written", "ko", "en", "set_at") VALUES ('Nuxt', '넉스트', '', 0), ('11월', '시비뤌', '', 0)"#,
    )
    .await
    .expect("an agreed reading is written");

  let (status, made) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-a", "text": "Nuxt and the Qwen model", "language": "ko" })),
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
  assert!(
    !prompts[0].contains("시비뤌"),
    "a row the check refuses is no agreed reading: {}",
    prompts[0]
  );
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
  assert!(slot.get("askedBy").is_none(), "a line naming no asker asks its slot for none: {slot}");
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

  let month: String = store.fetch_one(reading("11월").as_str()).await.expect("the row stands").get("ko");
  let misread: i64 = store
    .fetch_one(r#"SELECT count(*) AS "rows" FROM "voice_names" WHERE "written" = '12일'"#)
    .await
    .expect("the table is read")
    .get("rows");

  assert_eq!(month, "십일월", "a row the check refuses is written over by a reading that passes");
  assert_eq!(misread, 0, "a chosen reading that misreads its figures is not kept");

  let hashes: i64 = store
    .fetch_one(r#"SELECT count(*) AS "rows" FROM "voice_names" WHERE "written" = '7dae3520e72b'"#)
    .await
    .expect("the table is read")
    .get("rows");

  assert_eq!(hashes, 0, "an identifier is no name and gets no row");

  store.close().await.expect("the store connection closes");
}

// A line's asker reaches agency on every road a line comes in by, a restart's among them: the derivation of each runs
// to its end, so the runtime is let go without waiting for the card it then asks.
#[test]
fn the_asker_a_line_names_asks_its_slot_a_briefing_a_synthesis_and_a_line_taken_up_alike() {
  let runtime = tokio::runtime::Runtime::new().expect("a runtime");

  runtime.block_on(the_asker_asks_the_slot());
  runtime.shutdown_background();
}

async fn the_asker_asks_the_slot() {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());

  *asked.answers.lock().unwrap() =
    vec![r#"{"spoken": "퀜이 섰습니다.", "readings": []}"#.to_owned(); 3];

  let served = served(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await), QUICK).await;
  let engine = router(Served {
    context: Arc::clone(&served.context),
    events: served.events.clone(),
    token: served.token.clone(),
  });
  let line = json!({ "voiceName": "voice-a", "text": "Qwen is up.", "language": "ko" });
  let mut briefing = line.clone();

  briefing["chatId"] = json!(-5);
  briefing["kind"] = json!("summary");
  briefing["askedBy"] = json!("supervisor-a");

  let (status, briefed) = call(&engine, "POST", "/briefings", Some(briefing)).await;

  assert_eq!(status, StatusCode::CREATED, "{briefed}");

  let briefed = briefed["synthesisId"].as_i64().expect("its synthesis");
  let briefed = read_until(&engine, briefed, |read| read["spoken"].is_string()).await;
  let mut synthesis = line;

  synthesis["askedBy"] = json!("supervisor-b");

  let (status, made) = call(&engine, "POST", "/syntheses", Some(synthesis)).await;

  assert_eq!(status, StatusCode::CREATED, "{made}");
  assert_eq!(made["askedBy"], "supervisor-b", "the member says who its caller said asks it: {made}");

  read_until(&engine, made["id"].as_i64().expect("an id"), |read| read["spoken"].is_string()).await;

  let mut store = PgConnection::connect(&address_of(&scratch.name)).await.expect("the store answers");

  store
    .execute(
      r#"INSERT INTO "voice_syntheses" ("id", "voice_name", "text", "language", "asked_by", "state", "created_at")
         VALUES (900, 'voice-a', 'Qwen is up.', 'ko', 'supervisor-c', 'waiting', 0)"#,
    )
    .await
    .expect("a line a process left is written");
  store.close().await.expect("the store connection closes");
  bmo_voice::http::take_up(Arc::clone(&served.context), served.events.clone())
    .await
    .expect("the left line is taken up");
  read_until(&engine, 900, |read| read["spoken"].is_string()).await;

  let askers = asked
    .slots
    .lock()
    .unwrap()
    .iter()
    .map(|slot| slot["askedBy"].clone())
    .collect::<Vec<_>>();

  assert_eq!(briefed["askedBy"], "supervisor-a", "a briefing's asker is its synthesis's: {briefed}");
  assert_eq!(
    askers,
    [json!("supervisor-a"), json!("supervisor-b"), json!("supervisor-c")],
    "each slot is asked for the line's asker, handed on as given"
  );
}

#[tokio::test]
async fn a_turn_that_ended_with_no_answer_gives_its_session_up_and_the_next_try_stands_a_new_one() {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());
  let spoken = "넉스트 모델";

  *asked.answers.lock().unwrap() = vec![
    format!("{ENDED}the road answered nothing"),
    json!({ "spoken": spoken, "readings": [] }).to_string(),
  ];

  let engine = engine(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await)).await;
  let (status, made) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-a", "text": "the Nuxt model", "language": "ko" })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{made}");

  let id = made["id"].as_i64().expect("an id");
  // A second slot waits in its line and a second session is started before the try that answers.
  let read = read_within(&engine, id, 1200, |read| read["spoken"].is_string()).await;

  assert_eq!(read["spoken"], spoken, "{read}");
  assert_eq!(asked.prompts.lock().unwrap().len(), 2, "the try that did not answer is asked again");
  assert_eq!(asked.starts.lock().unwrap().len(), 2, "the next try stands on a session of its own");
  assert_eq!(
    *asked.ended.lock().unwrap(),
    ["transliterator@run:1", "transliterator@run:1"],
    "the session whose turn ended with no answer is ended, and the one that answered after it"
  );

  let given_back = asked.given_back.lock().unwrap().clone();

  assert_eq!(given_back.len(), 2, "each slot is given back: {given_back:?}");
  assert_eq!(given_back[0]["outcome"], "failed", "the first as failed: {given_back:?}");
}

#[tokio::test]
async fn a_line_whose_tries_are_spent_fails_saying_why_each_try_under_a_name_of_its_own() {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());

  *asked.answers.lock().unwrap() = vec!["no object".to_owned(); 3];

  let engine = engine(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await)).await;
  let (status, made) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-a", "text": "the Nuxt model", "language": "ko" })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{made}");

  let id = made["id"].as_i64().expect("an id");
  let read = read_until(&engine, id, |read| read["state"] == "failed").await;
  let failure = read["failure"].as_str().unwrap_or_default();

  assert!(
    failure.contains("did not stand after 3 tries") && failure.contains("held no JSON object"),
    "the failure says the tries are spent and why the last one failed: {read}"
  );
  assert_eq!(asked.prompts.lock().unwrap().len(), 3, "the policy's count of tries is asked, no more");

  let names = asked.names.lock().unwrap().clone();
  let prefix = format!("voice-synthesis-{id}-spoken-");

  assert!(names.iter().all(|name| name.starts_with(&prefix)), "{names:?}");
  assert_eq!(
    names.iter().collect::<std::collections::BTreeSet<_>>().len(),
    3,
    "each try goes out under a name no other try carries: {names:?}"
  );

  let given_back = asked.given_back.lock().unwrap().clone();

  assert_eq!(given_back.len(), 1, "the slot is given back once: {given_back:?}");
  assert_eq!(given_back[0]["outcome"], "failed", "{given_back:?}");
  assert_eq!(*asked.ended.lock().unwrap(), ["transliterator@run:1"], "and its session ended");
}

#[tokio::test]
async fn a_slot_the_pool_holds_back_tells_what_it_is_held_for_while_the_line_waits() {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());

  *asked.hold.lock().unwrap() = Some(
    "Nothing can carry the work on the road it was asked on.\nclaude-01 is at its limit until two oclock."
      .to_owned(),
  );

  // The policy's own pacing, whose first try still stands in the line while the test reads it.
  let engine =
    router(served(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await), policy_pacing()).await);
  let (status, made) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-a", "text": "Qwen is up." })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{made}");

  let id = made["id"].as_i64().expect("an id");
  let read = read_until(&engine, id, |read| {
    read["waitingOn"].as_str().is_some_and(|on| on.contains("at its limit"))
  })
  .await;
  let on = read["waitingOn"].as_str().unwrap_or_default();

  assert_eq!(read["state"], "waiting", "{read}");
  assert_eq!(read["spoken"], Value::Null);
  assert_eq!(read["wait"]["on"], "words", "the line still waits on its words: {read}");
  assert!(on.contains("waits in the role's line for a slot"), "{on}");
  assert!(on.contains("Nothing can carry the work on the road it was asked on"), "{on}");
  assert!(
    read["failure"].is_null(),
    "a line the pool has not yet refused keeps its reason as its wait, not as an end: {read}"
  );
}

#[tokio::test]
async fn a_slot_never_let_in_ends_the_line_naming_what_held_it_and_asks_one_slot_to_a_try() {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());

  *asked.hold.lock().unwrap() = Some("claude-01 is at its limit until two oclock.".to_owned());

  let engine = engine(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await)).await;
  let (status, made) = call(
    &engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-a", "text": "Qwen is up." })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{made}");

  let id = made["id"].as_i64().expect("an id");
  let read = read_until(&engine, id, |read| read["state"] == "failed").await;
  let failure = read["failure"].as_str().unwrap_or_default();

  assert!(failure.contains("did not stand after 3 tries"), "{read}");
  assert!(failure.contains("the pool did not let it in within"), "{failure}");
  assert!(failure.contains("claude-01 is at its limit"), "{failure}");

  let slots = asked.slots.lock().unwrap().clone();

  assert_eq!(slots.len(), 3, "one slot is asked to a try, and no more: {slots:?}");
  assert_eq!(
    slots
      .iter()
      .map(|slot| slot["requestKey"].as_str().unwrap_or_default())
      .collect::<std::collections::BTreeSet<_>>()
      .len(),
    3,
    "each try asks under a key of its own: {slots:?}"
  );
  assert!(
    asked.starts.lock().unwrap().is_empty(),
    "no session starts on a road that never let a slot in"
  );

  let given_back = asked.given_back.lock().unwrap().clone();

  assert_eq!(given_back.len(), 3, "each slot held back is given back: {given_back:?}");
  assert!(
    given_back
      .iter()
      .all(|given| given["outcome"] == "cancelled: not let in" && given["cancel"] == true),
    "cancelled, since no turn ran on any of them: {given_back:?}"
  );
}

#[tokio::test]
async fn a_briefing_never_voiced_carries_why_on_its_member_and_is_told_with_it() {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());

  *asked.answers.lock().unwrap() = vec!["no object".to_owned(); 3];

  let engine = engine(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await)).await;
  let (status, taken) = call(
    &engine,
    "POST",
    "/briefings",
    Some(
      json!({ "voiceName": "voice-a", "text": "The backup finished.", "language": "ko", "chatId": 7, "kind": "summary",
        "title": "The backup", "plays": false }),
    ),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{taken}");

  let id = taken["id"].as_i64().expect("an id");
  // Its end is kept and told once its three tries have failed, which takes the longer the busier the machine, so the
  // wait is on the told end itself, bounded.
  let mut told = None;

  for _ in 0..2_400 {
    let (_, page) = call(&engine, "GET", "/events?kinds=briefing.ended", None).await;

    told = page["items"]
      .as_array()
      .expect("a page")
      .iter()
      .find(|event| event["data"]["id"] == id)
      .cloned();

    if told.is_some() {
      break;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  let told = told.unwrap_or_else(|| panic!("briefing {id}'s end is not told"));
  let (_, over) = call(&engine, "GET", &format!("/briefings/{id}"), None).await;

  assert!(over["finishedAt"].is_i64(), "a told end is a kept end: {over}");
  assert_eq!(over["state"], "failed", "{over}");
  assert_eq!(
    over["text"].as_str(),
    Some("The backup finished."),
    "a briefing that was never voiced keeps its words"
  );

  let failure = over["failure"].as_str().unwrap_or_default();

  assert!(failure.contains("did not stand after 3 tries"), "{over}");

  assert_eq!(told["data"]["state"], "failed", "{told}");
  assert_eq!(
    told["data"]["failure"].as_str().unwrap_or_default(),
    failure,
    "the end an agent reads from its own wait is the end the stream tells"
  );
}

#[tokio::test]
async fn a_briefing_stopped_while_its_words_are_derived_gives_back_what_its_line_holds_of_agency() {
  let scratch = Scratch::new().await;
  let asked = Arc::new(Asked::default());

  *asked.answers.lock().unwrap() = vec!["no object".to_owned()];

  let agency = stand_in_agency(Arc::clone(&asked)).await;
  let engine = router(served(&scratch, Some(agency), policy_pacing()).await);
  let (status, briefing) = call(
    &engine,
    "POST",
    "/briefings",
    Some(json!({ "voiceName": "voice-a", "text": "the Nuxt model", "language": "ko", "chatId": 7, "kind": "summary", "plays": false })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{briefing}");

  let id = briefing["synthesisId"].as_i64().expect("its synthesis");

  // The first try's answer failed, and the next waits the policy's seconds: the stop comes in that wait.
  read_until(&engine, id, |read| {
    read["waitingOn"].as_str().is_some_and(|on| on.contains("asked again in"))
  })
  .await;

  let (status, stopped) = call(&engine, "POST", "/briefings/stop", Some(json!({ "chatId": 7 }))).await;

  assert_eq!(status, StatusCode::OK, "{stopped}");

  let owner = format!("voice:synthesis:{id}");

  for _ in 0..400 {
    if asked.owners_released.lock().unwrap().contains(&owner) {
      return;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  panic!("{owner} was never asked to give back what it holds");
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
  let served = served(&scratch, Some(stand_in_agency(Arc::clone(&asked)).await), QUICK).await;
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
  let served = served(&scratch, None, policy_pacing()).await;
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
