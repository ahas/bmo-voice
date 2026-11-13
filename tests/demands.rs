//! The card a line is spoken on (`docs/boundary-api.md`, 「The voice servers and the card」, 「bmo-arbiter」): a demand
//! for one card of `voice` granted at once, or waiting in the master's line however long it takes and then
//! granted, each reason it waits on told once; read again by its id where a read of every demand does not hold it,
//! and asked again only where the master answers `404` for it; found by its work key where its ask was not
//! answered; given back where the master refuses to read it; kept in the ledger while it is held; and given back as
//! the line ends, and at the next start where a process left it, in the ledger or only on the master.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
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

// The demands the stand-in is asked are numbered past it; those at or below it stood on it before the test.
const ASKED_PAST: i64 = 100;

/// How the stand-in master answers: what a demand asked is answered first, and what each read answers after, the
/// last standing for every read past them.
#[derive(Clone)]
enum Answer {
  Granted,
  Waiting,
  Gone,
}

/// The stand-in master: every demand it holds, by id and work key, and how its routes misbehave where a test asks.
#[derive(Default)]
struct Stand {
  asked: Mutex<Vec<Value>>,
  reads: Mutex<VecDeque<Answer>>,
  given: Mutex<Vec<i64>>,
  first: Mutex<Option<Answer>>,
  next_id: Mutex<i64>,
  live: Mutex<Vec<(i64, Option<String>)>>,
  // Each read of every demand leaves out those asked of it, as a read taken before they were asked does.
  omits: bool,
  // How many asks are taken in and answered 502, as an answer lost on its way.
  unanswered: Mutex<u32>,
  // Each read of every demand is refused.
  refuses: bool,
}

impl Stand {
  // The next answer a read takes.
  fn next(&self) -> Answer {
    let mut reads = self.reads.lock().unwrap();

    if reads.len() > 1 { reads.pop_front() } else { reads.front().cloned() }.unwrap_or(Answer::Granted)
  }

  // Every demand dropped, as a master restarted; the demands asked after are granted.
  fn drop_all(&self) {
    self.live.lock().unwrap().clear();
    *self.reads.lock().unwrap() = VecDeque::from([Answer::Granted]);
    *self.first.lock().unwrap() = Some(Answer::Granted);
  }

  fn holds(&self, id: i64) -> bool {
    self.live.lock().unwrap().iter().any(|(live, _)| *live == id)
  }
}

fn demand(id: i64, key: Option<&str>, answer: &Answer) -> Value {
  match answer {
    Answer::Granted => json!({
      "id": id, "serviceName": "voice", "count": 1, "state": "granted", "taken": null, "reason": null,
      "waitedMs": 0, "cards": [{ "nodeName": "node-a", "address": null, "keyFile": null }], "workKey": key
    }),
    _ => json!({
      "id": id, "serviceName": "voice", "count": 1, "state": "waiting", "taken": null, "cards": [],
      "reason": { "code": "in-use", "text": WAITING }, "waitedMs": 5, "workKey": key
    }),
  }
}

fn not_found() -> Response {
  (StatusCode::NOT_FOUND, Json(json!({ "code": "not-found", "error": "no such demand" }))).into_response()
}

async fn ask(State(stand): State<Arc<Stand>>, Json(body): Json<Value>) -> Response {
  let key = body["workKey"].as_str().map(str::to_owned);

  stand.asked.lock().unwrap().push(body);

  let id = {
    let mut next = stand.next_id.lock().unwrap();
    *next += 1;
    *next
  };

  stand.live.lock().unwrap().push((id, key.clone()));

  {
    let mut unanswered = stand.unanswered.lock().unwrap();

    if *unanswered > 0 {
      *unanswered -= 1;

      return (
        StatusCode::BAD_GATEWAY,
        Json(json!({ "code": "unreached", "error": "the answer was lost" })),
      )
        .into_response();
    }
  }

  let first = stand.first.lock().unwrap().clone().unwrap_or(Answer::Granted);
  let status = match first {
    Answer::Granted => StatusCode::CREATED,
    _ => StatusCode::ACCEPTED,
  };

  (status, Json(demand(id, key.as_deref(), &first))).into_response()
}

// One demand read by its id: `404` where the stand-in holds it no more, else the next answer.
async fn read(State(stand): State<Arc<Stand>>, Path(id): Path<i64>) -> Response {
  if !stand.holds(id) {
    return not_found();
  }

  let answer = stand.next();

  // A read is held open a moment, as the master holds a waiting one.
  tokio::time::sleep(Duration::from_millis(20)).await;

  if let Answer::Gone = answer {
    stand.drop_all();

    return not_found();
  }

  let key = stand
    .live
    .lock()
    .unwrap()
    .iter()
    .find(|(live, _)| *live == id)
    .and_then(|(_, key)| key.clone());

  Json(demand(id, key.as_deref(), &answer)).into_response()
}

// Every demand the stand-in holds, as one read answers them: each read takes the next answer, and a demand gone is
// one the page holds no more. A stand-in that omits leaves out every demand asked of it and takes no answer.
async fn list(State(stand): State<Arc<Stand>>) -> Response {
  if stand.refuses {
    return (StatusCode::BAD_REQUEST, Json(json!({ "code": "bad-request", "error": "refused" })))
      .into_response();
  }

  let live = stand.live.lock().unwrap().clone();

  if stand.omits {
    let items: Vec<Value> = live
      .iter()
      .filter(|(id, _)| *id <= ASKED_PAST)
      .map(|(id, key)| demand(*id, key.as_deref(), &Answer::Granted))
      .collect();

    return Json(json!({ "items": items, "more": false })).into_response();
  }

  let answer = stand.next();

  if let Answer::Gone = answer {
    stand.drop_all();

    return Json(json!({ "items": [], "more": false })).into_response();
  }

  let items: Vec<Value> = live.iter().map(|(id, key)| demand(*id, key.as_deref(), &answer)).collect();

  Json(json!({ "items": items, "more": false })).into_response()
}

async fn give_back(State(stand): State<Arc<Stand>>, Path(id): Path<i64>) -> StatusCode {
  stand.given.lock().unwrap().push(id);
  stand.live.lock().unwrap().retain(|(live, _)| *live != id);

  StatusCode::NO_CONTENT
}

/// The stand-in master serving on a port of its own, answering a demand first with `first` and its reads with
/// `reads`, as `stand` sets it otherwise.
async fn serve(stand: Stand, first: Answer, reads: Vec<Answer>) -> (Arc<Stand>, SocketAddr) {
  let stand = Arc::new(Stand {
    reads: Mutex::new(reads.into()),
    first: Mutex::new(Some(first)),
    next_id: Mutex::new(ASKED_PAST),
    ..stand
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

async fn master(first: Answer, reads: Vec<Answer>) -> (Arc<Stand>, SocketAddr) {
  serve(Stand::default(), first, reads).await
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
address = "http://[::1]:9"

[units.arbiter]
address = "http://{at}"

[units.web]
address = "http://[::1]:9"
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
async fn a_demand_the_master_answers_404_for_by_its_id_is_asked_again() {
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
async fn a_read_of_every_demand_that_leaves_out_a_waiting_one_never_asks_a_second() {
  let stand = Stand {
    omits: true,
    ..Stand::default()
  };
  let (stand, at) =
    serve(stand, Answer::Waiting, vec![Answer::Waiting, Answer::Waiting, Answer::Granted]).await;
  let (_root, settings) = settings(at);
  let units = Units::new(&settings).expect("the units are named");
  let held = card::hold(&units.arbiter, &settings.demands_folder(), &asking(), &mut |_| {})
    .await
    .expect("a card is granted");

  assert_eq!(held.demand, 101, "the demand read by its id is the one granted");
  assert_eq!(stand.asked.lock().unwrap().len(), 1, "no second demand is asked");
  assert!(stand.given.lock().unwrap().is_empty());
  assert_eq!(ledger(&settings), ["101"]);
}

#[tokio::test]
async fn an_ask_whose_answer_was_lost_is_found_by_its_work_key_not_asked_twice() {
  let stand = Stand {
    unanswered: Mutex::new(1),
    ..Stand::default()
  };
  let (stand, at) = serve(stand, Answer::Granted, vec![]).await;
  let (_root, settings) = settings(at);
  let units = Units::new(&settings).expect("the units are named");
  let mut told = vec![];
  let held = card::hold(&units.arbiter, &settings.demands_folder(), &asking(), &mut |why| {
    told.push(why.to_owned())
  })
  .await
  .expect("a card is granted");

  assert_eq!(held.demand, 101, "the demand the master took in is the line's");
  assert_eq!(stand.asked.lock().unwrap().len(), 1, "no second demand is asked");
  assert_eq!(told, [card::UNREACHED]);
  assert_eq!(ledger(&settings), ["101"]);
}

#[tokio::test]
async fn a_line_finding_two_demands_of_its_work_key_keeps_one() {
  let stand = Stand {
    live: Mutex::new(vec![(99, Some("voice:synthesis:7".to_owned()))]),
    unanswered: Mutex::new(1),
    ..Stand::default()
  };
  let (stand, at) = serve(stand, Answer::Granted, vec![]).await;
  let (_root, settings) = settings(at);
  let units = Units::new(&settings).expect("the units are named");
  let held = card::hold(&units.arbiter, &settings.demands_folder(), &asking(), &mut |_| {})
    .await
    .expect("a card is granted");

  assert_eq!(held.demand, 99, "the first of the line's work key is kept");
  assert_eq!(*stand.given.lock().unwrap(), [101], "the one beyond it is given back");
  assert_eq!(ledger(&settings), ["99"]);
}

#[tokio::test]
async fn a_refused_read_gives_the_demand_back_before_the_line_fails() {
  let stand = Stand {
    refuses: true,
    ..Stand::default()
  };
  let (stand, at) = serve(stand, Answer::Waiting, vec![]).await;
  let (_root, settings) = settings(at);
  let units = Units::new(&settings).expect("the units are named");
  let held = card::hold(&units.arbiter, &settings.demands_folder(), &asking(), &mut |_| {}).await;

  assert!(held.is_err(), "a refusal of the master's ends the wait: {held:?}");
  assert_eq!(*stand.given.lock().unwrap(), [101]);
  assert!(ledger(&settings).is_empty());
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
      ask: &ask,
      service_within: Duration::from_secs(60),
      answer_within: Duration::from_secs(60),
      still_within: Duration::from_secs(600),
    },
    &mut |stage| {
      stages.push(match stage {
        Stage::Waiting(why) => format!("waiting: {why}"),
        Stage::OnNode {
          demand,
          node,
          stage,
        } => format!("on {node} by {demand}: {}", stage.stage),
      })
    },
  )
  .await;

  assert!(
    matches!(spoken, Err(NotSpoken::OnNode(_, _))),
    "the granted node is out of reach, so the line fails: {spoken:?}"
  );
  assert_eq!(stages, [format!("waiting: {WAITING}"), format!("on node-a by 101: {}", node::ASKED)]);
  assert_eq!(*stand.given.lock().unwrap(), [101], "the card is given back as the line ends");
  assert!(ledger(&settings).is_empty());
}

#[tokio::test]
async fn the_demands_a_process_left_are_given_back_at_the_next_start_from_the_ledger_and_the_master() {
  let left = |id: i64, key: &str| (id, Some(key.to_owned()));
  let stand = Stand {
    live: Mutex::new(vec![
      left(41, "voice:synthesis:5"),
      left(43, "voice:synthesis:9"),
      left(44, "research:run:1"),
      (45, None),
    ]),
    ..Stand::default()
  };
  let (stand, at) = serve(stand, Answer::Granted, vec![]).await;
  let (_root, settings) = settings(at);
  let units = Units::new(&settings).expect("the units are named");
  let folder = settings.demands_folder();

  std::fs::create_dir_all(&folder).expect("the ledger is made");
  std::fs::write(folder.join("41"), "").expect("a demand is left");
  std::fs::write(folder.join("42"), "").expect("a demand is left");

  let given = card::give_back_left(&units.arbiter, &folder, node::WORK).await;
  let mut back = stand.given.lock().unwrap().clone();

  back.sort_unstable();

  assert_eq!(given, 3);
  assert_eq!(
    back,
    [41, 42, 43],
    "the ledger's and the master's demands of the voice's work, and no other"
  );
  assert!(ledger(&settings).is_empty());
}
