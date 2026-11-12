//! The clocks that end a synthesis and what a synthesis waits on (`docs/boundary-api.md`, 「Its states」, 「What it
//! waits on」, 「The clocks」): one whose caller stops reading it fails as its caller gone, naming its demand while it
//! waits; one whose node does not answer fails naming the node; and one that stands still on its node names the node
//! and its stage while it waits, then fails naming where it stood, its call ended on the node. Each gives its card
//! back. The clocks are set short, so only the seconds go.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::StatusCode;
use bmo_voice::context::Clocks;
use bmo_voice::http::router;
use bmo_voice::store;
use bmo_voice::wire::SynthesisState;
use serde_json::{Value, json};

use super::briefings::{
  Cards, IN_USE, Line, SPOKEN, Scratch, call, cards_master, line_master, served_with,
};

const LONG: Duration = Duration::from_secs(600);

/// What a stand-in hub does with the commands it is handed.
#[derive(Clone, Copy)]
enum Hub {
  /// It never answers a command.
  Silent,
  /// It starts the call, and every look at it finds the line waiting at the same stage, nothing on the node moving.
  Still,
}

/// The stand-in hub's commands, kept as they came.
#[derive(Default)]
struct Heard {
  commands: Mutex<Vec<String>>,
}

const STAGE: &str = r#"{"stage": "room", "step": 2, "said": "waits for room on the card for the model"}"#;

// One command's run as the hub streams it: its standard output, then its exit.
fn ran(stdout: &str) -> String {
  use base64::Engine;

  let chunk = base64::engine::general_purpose::STANDARD.encode(stdout);

  format!(
    "event: stdout\ndata: {}\n\nevent: exit\ndata: {}\n\n",
    json!({ "chunk": { "base64": chunk } }),
    json!({ "code": 0 })
  )
}

async fn hub(behaves: Hub, heard: Arc<Heard>) -> SocketAddr {
  use axum::extract::State;
  use axum::routing::post;

  let router = axum::Router::new()
    .route(
      "/exec",
      post(|State((behaves, heard)): State<(Hub, Arc<Heard>)>, axum::Json(asked): axum::Json<Value>| async move {
        let command = asked["command"].as_str().unwrap_or_default().to_owned();

        heard.commands.lock().unwrap().push(command.clone());

        if let Hub::Silent = behaves {
          std::future::pending::<()>().await;
        }

        let stdout = if command.contains("setsid") {
          "/stand-in/spoken\n".to_owned()
        } else if command.contains("flock -w") {
          // A look waits on the node a moment before it answers, as the node's own wait does.
          tokio::time::sleep(Duration::from_millis(20)).await;
          format!("waiting\nstage {STAGE}\nmoved 1000.5\n")
        } else {
          String::new()
        };

        ([("content-type", "text/event-stream")], ran(&stdout))
      }),
    )
    .with_state((behaves, heard));
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a port");
  let at = listener.local_addr().expect("its address");

  tokio::spawn(async move { axum::serve(listener, router).await });

  at
}

/// A stand-in node service: every node is a hub node, reached through the hub at `hub` with the token in `token_file`.
async fn hub_nodes(hub: SocketAddr, token_file: std::path::PathBuf) -> SocketAddr {
  use axum::extract::{Path, State};
  use axum::routing::get;

  let router = axum::Router::new()
    .route(
      "/nodes/{name}/target",
      get(
        |State((hub, token_file)): State<(SocketAddr, String)>, Path(name): Path<String>| async move {
          axum::Json(
            json!({ "nodeName": name, "kind": "hub", "role": "voice", "hubAddress": format!("http://{hub}"),
          "hubTokenFile": token_file, "agentName": name }),
          )
        },
      ),
    )
    .with_state((hub, token_file.to_string_lossy().into_owned()));
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a port");
  let at = listener.local_addr().expect("its address");

  tokio::spawn(async move { axum::serve(listener, router).await });

  at
}

/// The engine on a scratch store whose master grants the first synthesis its card on `node-a`, a hub node reached
/// through a stand-in hub that behaves as `behaves`, its clocks those `clocks` sets.
async fn granted_on_a_hub(
  scratch: &Scratch,
  behaves: Hub,
  clocks: Clocks,
) -> (axum::Router, Arc<Cards>, Arc<Heard>) {
  let cards = Arc::new(Cards {
    granted: "voice:synthesis:1".to_owned(),
    asked: Default::default(),
    given: Default::default(),
  });
  let heard = Arc::new(Heard::default());
  let token_file = scratch.root.join("hub.env");

  std::fs::write(&token_file, "JAKE_CONTROL_TOKEN=a-hub-token\n").expect("the hub's token is written");

  let master = cards_master(Arc::clone(&cards)).await;
  let nodes = hub_nodes(hub(behaves, Arc::clone(&heard)).await, token_file).await;
  let served = served_with(scratch, Some(master), Some(nodes), clocks).await;

  (router(served), cards, heard)
}

async fn synthesis(engine: &axum::Router) -> i64 {
  let (status, made) = call(
    engine,
    "POST",
    "/syntheses",
    Some(json!({ "voiceName": "voice-b", "text": "A short line.", "spoken": SPOKEN })),
  )
  .await;

  assert_eq!(status, StatusCode::CREATED, "{made}");

  made["id"].as_i64().expect("an id")
}

/// The synthesis read by its caller until `until` holds of it.
async fn read_until(engine: &axum::Router, id: i64, until: impl Fn(&Value) -> bool) -> Value {
  let mut read = Value::Null;

  for _ in 0..400 {
    (_, read) = call(engine, "GET", &format!("/syntheses/{id}"), None).await;

    if until(&read) {
      return read;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  panic!("synthesis {id} never came to it: {read}");
}

/// The card given back once, as the line ends.
async fn given_back(given: &Mutex<Vec<i64>>, demand: i64) {
  for _ in 0..400 {
    if given.lock().unwrap().contains(&demand) {
      return;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  panic!("the card {demand} was never given back");
}

#[tokio::test]
async fn a_synthesis_whose_caller_stops_reading_it_fails_as_its_caller_gone_and_gives_its_card_back() {
  let scratch = Scratch::new().await;
  let line = Arc::new(Line::default());
  let clocks = Clocks {
    caller_gone: Duration::from_millis(300),
    node_answer: LONG,
    node_still: LONG,
  };
  let served = served_with(&scratch, Some(line_master(Arc::clone(&line)).await), None, clocks).await;
  let db = served.context.db.clone();
  let engine = router(served);
  let id = synthesis(&engine).await;

  // While its caller reads it, it waits on its demand and says why.
  let read = read_until(&engine, id, |read| read["wait"]["on"] == "demand").await;

  assert_eq!(read["state"], "waiting", "{read}");
  assert_eq!(read["waitingOn"], IN_USE, "{read}");
  assert_eq!(
    (&read["wait"]["node"], &read["wait"]["demandId"]),
    (&Value::Null, &Value::Null),
    "{read}"
  );

  // Its caller goes: the store is looked at, which reads nothing for the caller.
  let mut row = None;

  for _ in 0..400 {
    row = store::syntheses::get(&db, id).await.expect("the store answers");

    if row.as_ref().is_some_and(|row| row.state != SynthesisState::Waiting) {
      break;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  let row = row.expect("the synthesis stands");

  assert_eq!(row.state, SynthesisState::Failed);
  assert_eq!(row.failure.as_deref(), Some("its caller has gone, not having read it for 0.3 s"));
  given_back(&line.given, 9).await;

  let (_, read) = call(&engine, "GET", &format!("/syntheses/{id}"), None).await;

  assert_eq!(
    (&read["wait"], &read["waitingOn"]),
    (&Value::Null, &Value::Null),
    "an ended synthesis waits on nothing"
  );
}

#[tokio::test]
async fn a_synthesis_whose_node_does_not_answer_fails_naming_the_node_and_gives_its_card_back() {
  let scratch = Scratch::new().await;
  let clocks = Clocks {
    caller_gone: LONG,
    node_answer: Duration::from_millis(300),
    node_still: LONG,
  };
  let (engine, cards, heard) = granted_on_a_hub(&scratch, Hub::Silent, clocks).await;
  let id = synthesis(&engine).await;
  let read = read_until(&engine, id, |read| read["state"] == "failed").await;

  assert_eq!(
    read["failure"], "on node-a: voice node unreachable: it did not answer within 0.3 s",
    "{read}"
  );
  assert!(
    heard.commands.lock().unwrap().iter().any(|command| command.contains("setsid")),
    "the line was asked"
  );
  given_back(&cards.given, 100).await;
}

#[tokio::test]
async fn a_line_standing_still_on_its_node_names_the_node_and_its_stage_then_fails_and_is_ended_there() {
  let scratch = Scratch::new().await;
  let clocks = Clocks {
    caller_gone: LONG,
    node_answer: LONG,
    node_still: Duration::from_millis(400),
  };
  let (engine, cards, heard) = granted_on_a_hub(&scratch, Hub::Still, clocks).await;
  let id = synthesis(&engine).await;

  // Granted its card, the line stands on its node, and the synthesis says which and where.
  let read = read_until(&engine, id, |read| {
    read["wait"]["on"] == "node" && read["waitingOn"] != "node-a: the line is asked of the node"
  })
  .await;

  assert_eq!(read["state"], "speaking", "{read}");
  assert_eq!(
    (&read["wait"]["node"], &read["wait"]["demandId"]),
    (&json!("node-a"), &json!(100)),
    "{read}"
  );
  assert_eq!(read["waitingOn"], "node-a: waits for room on the card for the model", "{read}");
  assert!(read["wait"]["since"].is_i64(), "{read}");

  // Nothing on the node moves: the line ends on the node's progress clock, its asker ended there.
  let read = read_until(&engine, id, |read| read["state"] == "failed").await;

  assert_eq!(
    read["failure"],
    "on node-a: voice node failed: the line stood still for 0.4 s at: waits for room on the card for the model",
    "{read}"
  );
  given_back(&cards.given, 100).await;

  for _ in 0..400 {
    if heard.commands.lock().unwrap().iter().any(|command| command.contains("kill -TERM")) {
      return;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  panic!("the line's asker was never ended on its node");
}
