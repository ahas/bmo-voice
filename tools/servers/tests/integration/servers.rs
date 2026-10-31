//! The voice servers said as text (docs/boundary-api.md, 「The hosted tool」): one line a server in the node
//! list's order, then where a line is spoken now (docs/voice-plan.md, 「Which road speaks a line」), and a
//! failure said as the harness plugin says it; each built from a page standing in for the engine's answer.

use bmo_voice::client::ClientFailure;
use bmo_voice::wire::{Page, VoiceServer, WireError};
use serde_json::{Value, json};
use voice_servers_tool::{failure_text, servers_text};

fn server(node_name: &str, state: &str, voice_names: &[&str]) -> VoiceServer {
  VoiceServer {
    node_name: node_name.to_owned(),
    state: state.to_owned(),
    voice_names: voice_names.iter().map(|name| (*name).to_owned()).collect(),
  }
}

// The page as the engine answers it on the wire, read back into its shape.
fn page(servers: Value) -> Page<VoiceServer> {
  serde_json::from_value(json!({ "items": servers, "more": false })).unwrap()
}

// "each server is a line in the list's order, with its state and its loaded voices, then the road"
#[test]
fn each_server_is_a_line_in_the_lists_order() {
  let page = page(json!([
    { "nodeName": "n2", "state": "running", "voiceNames": ["alpha", "beta"] },
    { "nodeName": "n5", "state": "running", "voiceNames": [] },
    { "nodeName": "n3", "state": "absent", "voiceNames": [] },
  ]));

  assert_eq!(
    servers_text(&page.items).lines().collect::<Vec<_>>(),
    [
      "n2: running; voices loaded: alpha, beta",
      "n5: running; voices loaded: none",
      "n3: absent; voices loaded: none",
      "Where a line is spoken now: on the voice server of the listed node the arbiter's master grants a card of voice.",
    ]
  );
}

// "a list of absent servers alone still says a line is spoken on the listed node the master grants"
#[test]
fn absent_servers_alone_still_name_the_listed_road() {
  let text = servers_text(&[server("n4", "absent", &[])]);

  assert!(text.starts_with("n4: absent; voices loaded: none\n"), "{text}");
  assert!(text.ends_with("the arbiter's master grants a card of voice."), "{text}");
}

// "an empty list says a synthesis fails and nothing speaks on the operations machine's card"
#[test]
fn an_empty_list_says_nothing_speaks() {
  let text = servers_text(&page(json!([])).items);

  assert_eq!(text.lines().count(), 1, "{text}");
  assert!(text.starts_with("Where a line is spoken now: nowhere."), "{text}");
  assert!(text.contains("No voice node is listed"), "{text}");
  assert!(text.contains("a synthesis fails"), "{text}");
  assert!(text.contains("nothing speaks on the operations machine's card"), "{text}");
}

// "a refusal carries its status, code and sentence; no answer carries the sentence saying why"
#[test]
fn a_failure_is_said_as_the_plugins_are() {
  let refused = ClientFailure::Answered {
    status: 401,
    answer: Some(WireError {
      code: "unauthorized".to_owned(),
      error: "the token is missing or wrong".to_owned(),
    }),
    body: String::new(),
  };
  let bare = ClientFailure::Answered {
    status: 502,
    answer: None,
    body: "Bad Gateway".to_owned(),
  };
  let unreached = ClientFailure::Unreached("connection refused".to_owned());
  let read = |failure: &ClientFailure| serde_json::from_str::<Value>(&failure_text(failure)).unwrap();

  assert_eq!(
    read(&refused),
    json!({ "status": 401, "code": "unauthorized", "error": "the token is missing or wrong" })
  );
  assert_eq!(read(&bare)["status"], 502);
  assert!(read(&bare)["error"].as_str().unwrap().contains("Bad Gateway"));
  assert!(read(&unreached)["error"].as_str().unwrap().contains("could not be reached"));
  assert!(read(&unreached).get("status").is_none());
}
