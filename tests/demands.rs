//! The card a line is spoken on (`docs/boundary-api.md`, 「The voice servers and the card」, 「bmo-arbiter」): a demand
//! for one card of `voice` granted at once, or waiting in the master's line however long it takes and then
//! granted, each reason it waits on told once; asked again where the master no longer holds it; kept in the ledger
//! while it is held; and given back as the line ends, and at the next start where a process left it.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use bmo_voice::settings::Settings;
use bmo_voice::speech::card::{self, Held};
use bmo_voice::speech::node::{self, NotSpoken, Speech, Stage};
use bmo_voice::store::voices::VoiceRow;
use bmo_voice::units::Units;
use bmo_voice::units::arbiter::Ask;
use serde_json::{Value, json};

static MADE: AtomicU32 = AtomicU32::new(0);

const WAITING: &str = "Every card of voice is in use: song.";

/// How the stand-in master answers: what a demand asked is answered first, and what each read answers after, the
/// last standing for every read past them.
#[derive(Clone)]
enum Answer {
  Granted,
  Waiting,
  Gone,
}

#[derive(Default)]
struct Stand {
  asked: Mutex<Vec<Value>>,
  reads: Mutex<VecDeque<Answer>>,
  given: Mutex<Vec<i64>>,
  first: Mutex<Option<Answer>>,
  next_id: Mutex<i64>,
  live: Mutex<Vec<i64>>,
}

fn demand(id: i64, answer: &Answer) -> Value {
  match answer {
    Answer::Granted => json!({
      "id": id, "serviceName": "voice", "count": 1, "state": "granted", "taken": null, "reason": null,
      "waitedMs": 0, "cards": [{ "nodeName": "node-a", "address": null, "keyFile": null }]
    }),
    _ => json!({
      "id": id, "serviceName": "voice", "count": 1, "state": "waiting", "taken": null, "cards": [],
      "reason": { "code": "in-use", "text": WAITING }, "waitedMs": 5
    }),
  }
}

async fn ask(State(stand): State<Arc<Stand>>, Json(body): Json<Value>) -> impl IntoResponse {
  stand.asked.lock().unwrap().push(body);

  let id = {
    let mut next = stand.next_id.lock().unwrap();
    *next += 1;
    *next
  };

  stand.live.lock().unwrap().push(id);
  let first = stand.first.lock().unwrap().clone().unwrap_or(Answer::Granted);
  let status = match first {
    Answer::Granted => StatusCode::CREATED,
    _ => StatusCode::ACCEPTED,
  };

  (status, Json(demand(id, &first)))
}

async fn read(State(stand): State<Arc<Stand>>, Path(id): Path<i64>) -> impl IntoResponse {
  let answer = {
    let mut reads = stand.reads.lock().unwrap();

    if reads.len() > 1 { reads.pop_front() } else { reads.front().cloned() }.unwrap_or(Answer::Granted)
  };

  // A read is held open a moment, as the master holds a waiting one.
  tokio::time::sleep(Duration::from_millis(20)).await;

  match answer {
    Answer::Gone => {
      // Once gone, the demand asked again is granted.
      *stand.reads.lock().unwrap() = VecDeque::from([Answer::Granted]);
      *stand.first.lock().unwrap() = Some(Answer::Granted);

      (StatusCode::NOT_FOUND, Json(json!({ "code": "not-found", "error": "no such demand" })))
    }
    answer => (StatusCode::OK, Json(demand(id, &answer))),
  }
}

// Every demand the stand-in holds, as one read answers them: each read takes the next answer, and a demand gone is
// one the page holds no more.
async fn list(State(stand): State<Arc<Stand>>) -> impl IntoResponse {
  let answer = {
    let mut reads = stand.reads.lock().unwrap();

    if reads.len() > 1 { reads.pop_front() } else { reads.front().cloned() }.unwrap_or(Answer::Granted)
  };
  let live = stand.live.lock().unwrap().clone();

  if let Answer::Gone = answer {
    // Once gone, the demand asked again is granted.
    stand.live.lock().unwrap().clear();
    *stand.reads.lock().unwrap() = VecDeque::from([Answer::Granted]);
    *stand.first.lock().unwrap() = Some(Answer::Granted);

    return Json(json!({ "items": [], "more": false }));
  }

  Json(json!({ "items": live.iter().map(|id| demand(*id, &answer)).collect::<Vec<_>>(), "more": false }))
}

async fn give_back(State(stand): State<Arc<Stand>>, Path(id): Path<i64>) -> StatusCode {
  stand.given.lock().unwrap().push(id);
  stand.live.lock().unwrap().retain(|live| *live != id);

  StatusCode::NO_CONTENT
}

/// The stand-in master serving on a port of its own, answering a demand first with `first` and its reads with
/// `reads`.
async fn master(first: Answer, reads: Vec<Answer>) -> (Arc<Stand>, SocketAddr) {
  let stand = Arc::new(Stand {
    reads: Mutex::new(reads.into()),
    first: Mutex::new(Some(first)),
    next_id: Mutex::new(100),
    ..Stand::default()
  });
  let router = Router::new()
    .route("/demands", post(ask).get(list))
    .route("/demands/{id}", get(read))
    .route("/demands/{id}", delete(give_back))
    .with_state(Arc::clone(&stand));
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a port");
  let at = listener.local_addr().expect("its address");

  tokio::spawn(async move { axum::serve(listener, router).await });

  (stand, at)
}

/// A data root of this test's own, with settings naming the stand-in master and a node service nothing answers.
struct Root(std::path::PathBuf);

impl Drop for Root {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

fn settings(at: SocketAddr) -> (Root, Settings) {
  let made = MADE.fetch_add(1, Ordering::SeqCst);
  let root = std::env::temp_dir().join(format!("bmo_voice_demands_test_{}_{made}", std::process::id()));

  std::fs::create_dir_all(root.join("env")).expect("the data root is made");

  let path = root.join("env").join("voice.toml");

  std::fs::write(
    &path,
    format!(
      r#"listen = "127.0.0.1:0"
address = "http://127.0.0.1:0"
token_file = "voice.token"
database = "postgres:///unused"
data_root = "{}"

[units.node]
address = "http://127.0.0.1:9"

[units.arbiter]
address = "http://{at}"

[units.web]
address = "http://127.0.0.1:9"
"#,
      root.display()
    ),
  )
  .expect("the settings are written");

  let settings = Settings::read(&path).expect("the settings read");

  (Root(root), settings)
}

fn asking() -> Ask {
  Ask {
    service_name: "voice".to_owned(),
    count: 1,
    work_key: Some("voice:synthesis:7".to_owned()),
    reference: Some("voice synthesis 7 in voice-a".to_owned()),
  }
}

fn ledger(settings: &Settings) -> Vec<String> {
  let mut held: Vec<String> = std::fs::read_dir(settings.demands_folder())
    .map(|entries| {
      entries
        .filter_map(Result::ok)
        .map(|one| one.file_name().to_string_lossy().into_owned())
        .collect()
    })
    .unwrap_or_default();

  held.sort();
  held
}

#[tokio::test]
async fn a_demand_granted_at_once_names_its_node_stands_in_the_ledger_and_is_given_back() {
  let (stand, at) = master(Answer::Granted, vec![]).await;
  let (_root, settings) = settings(at);
  let units = Units::new(&settings).expect("the units are named");
  let mut told = vec![];
  let held = card::hold(&units.arbiter, &settings.demands_folder(), &asking(), &mut |why| {
    told.push(why.to_owned())
  })
  .await
  .expect("a card is granted");

  assert_eq!(
    held,
    Held {
      demand: 101,
      node: "node-a".to_owned()
    }
  );
  assert!(told.is_empty(), "a demand granted at once waits on nothing: {told:?}");
  assert_eq!(ledger(&settings), ["101"]);

  let asked = stand.asked.lock().unwrap().clone();

  assert_eq!(
    asked,
    [
      json!({ "serviceName": "voice", "count": 1, "workKey": "voice:synthesis:7", "ref": "voice synthesis 7 in voice-a" })
    ]
  );

  card::give_back(&units.arbiter, &settings.demands_folder(), held.demand).await;

  assert_eq!(*stand.given.lock().unwrap(), [101]);
  assert!(ledger(&settings).is_empty());
}

#[tokio::test]
async fn a_waiting_demand_tells_why_once_and_is_held_until_its_grant() {
  let (stand, at) = master(
    Answer::Waiting,
    vec![Answer::Waiting, Answer::Waiting, Answer::Waiting, Answer::Granted],
  )
  .await;
  let (_root, settings) = settings(at);
  let units = Units::new(&settings).expect("the units are named");
  let mut told = vec![];
  let held = card::hold(&units.arbiter, &settings.demands_folder(), &asking(), &mut |why| {
    told.push(why.to_owned())
  })
  .await
  .expect("a card is granted");

  assert_eq!(held.node, "node-a");
  assert_eq!(told, [WAITING], "the reason is told once, as it changes");
  assert!(stand.given.lock().unwrap().is_empty(), "nothing is given back while the card is held");
}

#[tokio::test]
async fn a_demand_the_master_no_longer_holds_is_asked_again() {
  let (stand, at) = master(Answer::Waiting, vec![Answer::Gone]).await;
  let (_root, settings) = settings(at);
  let units = Units::new(&settings).expect("the units are named");
  let held = card::hold(&units.arbiter, &settings.demands_folder(), &asking(), &mut |_| {})
    .await
    .expect("a card is granted");

  assert_eq!(held.demand, 102, "the second demand is the one granted");
  assert_eq!(stand.asked.lock().unwrap().len(), 2);
  assert_eq!(ledger(&settings), ["102"], "the demand gone is out of the ledger");
}

#[tokio::test]
async fn a_line_gives_its_card_back_however_it_ends_and_says_each_stage() {
  let (stand, at) = master(Answer::Waiting, vec![Answer::Granted]).await;
  let (_root, settings) = settings(at);
  let units = Units::new(&settings).expect("the units are named");
  let voices = settings.voices_folder();
  let voice = VoiceRow {
    name: "voice-a".to_owned(),
    language: "ko".to_owned(),
    set_at: 0,
    checkpoint: voices.join("voice-a").to_string_lossy().into_owned(),
    reference: voices.join("reference/voice-a.wav").to_string_lossy().into_owned(),
    cfg: 2.0,
    steps: 10,
    style: String::new(),
    seed: 0,
    prompts: vec![],
    by_sentence: false,
  };
  let out = settings.voice_folder().join("syntheses/7.ogg");
  let ask = asking();
  let mut stages = vec![];
  let spoken = node::speak_on_card(
    Speech {
      units: &units,
      settings: &settings,
      voice: &voice,
      text: "A short line.",
      out: &out,
      cap_ms: 60_000,
      ask: &ask,
    },
    &mut |stage| {
      stages.push(match stage {
        Stage::Waiting(why) => format!("waiting: {why}"),
        Stage::Speaking => "speaking".to_owned(),
      })
    },
  )
  .await;

  assert!(
    matches!(spoken, Err(NotSpoken::Node(_))),
    "the granted node is out of reach, so the line fails: {spoken:?}"
  );
  assert_eq!(stages, [format!("waiting: {WAITING}"), "speaking".to_owned()]);
  assert_eq!(*stand.given.lock().unwrap(), [101], "the card is given back as the line ends");
  assert!(ledger(&settings).is_empty());
}

#[tokio::test]
async fn the_demands_a_process_left_are_given_back_at_the_next_start() {
  let (stand, at) = master(Answer::Granted, vec![]).await;
  let (_root, settings) = settings(at);
  let units = Units::new(&settings).expect("the units are named");
  let folder = settings.demands_folder();

  std::fs::create_dir_all(&folder).expect("the ledger is made");
  std::fs::write(folder.join("41"), "").expect("a demand is left");
  std::fs::write(folder.join("42"), "").expect("a demand is left");

  let given = card::give_back_left(&units.arbiter, &folder).await;
  let mut back = stand.given.lock().unwrap().clone();

  back.sort_unstable();

  assert_eq!(given, 2);
  assert_eq!(back, [41, 42]);
  assert!(ledger(&settings).is_empty());
}
