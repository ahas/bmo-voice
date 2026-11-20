//! The request the tool host hands the executable and the answer it reads back (bmo-pocket's
//! `docs/mcp-host.md`, 「A call」): protocol 1, a call of the one entry, and an answer of text pieces.

use serde_json::json;
use voice_servers_tool::{Asked, ENTRY, GENERATE_ENTRY, SOUND_ENTRY, read_arguments, read_handed};

use crate::stand::{call, call_entry, env_without_settings, text, tool};

// "a call is read with its entry and arguments, the record and the sockets beside it passed over"
#[test]
fn a_call_is_read_with_its_entry_and_arguments() {
  let handed = read_handed(call(json!({})).to_string().as_bytes()).unwrap();

  assert_eq!(handed.protocol, 1);
  assert!(matches!(&handed.asked, Asked::Call { entry, .. } if entry == ENTRY));
  assert_eq!(handed.session["holderName"], "producer");
}

// "the resources kinds read, and a kind the tool does not answer reads as another"
#[test]
fn the_resources_kinds_and_other_kinds_read() {
  let listed =
    read_handed(json!({ "protocol": 1, "kind": "resources_list" }).to_string().as_bytes()).unwrap();
  let read = read_handed(
    json!({ "protocol": 1, "kind": "resources_read", "uri": "voice://servers", "session": {} })
      .to_string()
      .as_bytes(),
  )
  .unwrap();
  let claimed = read_handed(
    json!({ "protocol": 1, "kind": "claim", "url": "https://example.com", "session": {} })
      .to_string()
      .as_bytes(),
  )
  .unwrap();

  assert_eq!(listed.asked, Asked::ResourcesList);
  assert!(matches!(&read.asked, Asked::ResourcesRead { uri } if uri == "voice://servers"));
  assert_eq!(claimed.asked, Asked::Other);
}

// "a request in another protocol, or no request at all, does not read"
#[test]
fn a_request_in_another_protocol_does_not_read() {
  let mut request = call(json!({}));

  request["protocol"] = json!(2);

  let refusal = read_handed(request.to_string().as_bytes()).unwrap_err();

  assert!(refusal.contains("protocol 2"), "{refusal}");
  assert!(read_handed("not json".as_bytes()).unwrap_err().contains("does not read"));
}

// "the entry takes no arguments: an empty object or none, and nothing else"
#[test]
fn the_entry_takes_no_arguments() {
  assert!(read_arguments(&json!({})).is_ok());
  assert!(read_arguments(&json!(null)).is_ok());
  assert!(read_arguments(&json!({ "node": "n2" })).unwrap_err().contains("node"));
  assert!(read_arguments(&json!([])).is_err());
}

// "a call of another entry, or with arguments, answers an error, and no settings are read"
#[test]
fn a_call_of_another_entry_or_with_arguments_answers_an_error() {
  let (env, missing) = env_without_settings("other-entry");
  let mut other = call(json!({}));

  other["entry"] = json!("voice/other");

  for (request, said) in [(other, ENTRY), (call(json!({ "node": "n2" })), "takes no arguments")] {
    let ran = tool(&request.to_string(), &env);
    let answer = ran.answer();

    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(answer["isError"], true);
    assert!(text(&answer).contains(said), "{}", text(&answer));
    assert!(!text(&answer).contains(&missing), "{}", text(&answer));
  }
}

// "a call of the sound entry with no arguments answers with what it takes, and reads no settings"
#[test]
fn the_sound_entry_says_what_it_takes() {
  let (env, missing) = env_without_settings("sound-arguments");

  for (arguments, said) in [
    (json!({}), "synthesisId"),
    (json!({ "synthesisId": 1, "key": "", "path": "/tmp/line.ogg" }), "non-blank key"),
    (json!({ "synthesisId": 1, "key": "line", "path": "line.ogg" }), "absolute path"),
  ] {
    let ran = tool(&call_entry(SOUND_ENTRY, arguments).to_string(), &env);
    let answer = ran.answer();

    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(answer["isError"], true);
    assert!(text(&answer).contains(said), "{}", text(&answer));
    assert!(!text(&answer).contains(&missing), "{}", text(&answer));
  }
}

// "a sound call with arguments answers the settings it cannot read, and not the arguments"
#[test]
fn a_sound_call_reads_the_settings() {
  let (env, missing) = env_without_settings("sound-settings");
  let arguments = json!({ "synthesisId": 1, "key": "line", "path": "/tmp/line.ogg" });
  let ran = tool(&call_entry(SOUND_ENTRY, arguments).to_string(), &env);
  let answer = ran.answer();

  assert_eq!(ran.code, 0, "{}", ran.err);
  assert_eq!(answer["isError"], true);
  assert!(text(&answer).contains("settings do not read"), "{}", text(&answer));
  assert!(text(&answer).contains(&missing), "{}", text(&answer));
}

// "the executable answers the generate entry: arguments that do not read before any settings, then the settings"
#[test]
fn the_generate_entry_is_answered() {
  let (env, missing) = env_without_settings("generate");

  for (arguments, said) in [
    (json!({ "text": "hi" }), "voiceName"),
    (json!({ "voiceName": "alpha", "text": "hi" }), "settings do not read"),
  ] {
    let ran = tool(&call_entry(GENERATE_ENTRY, arguments).to_string(), &env);
    let answer = ran.answer();

    assert_eq!(ran.code, 0, "{}", ran.err);
    assert_eq!(answer["isError"], true);
    assert!(text(&answer).contains(said), "{}", text(&answer));
    assert_eq!(text(&answer).contains(&missing), said == "settings do not read", "{}", text(&answer));
  }
}

// "settings that do not read answer an error naming their path"
#[test]
fn settings_that_do_not_read_answer_an_error() {
  let (env, missing) = env_without_settings("unread");
  let ran = tool(&call(json!({})).to_string(), &env);
  let answer = ran.answer();

  assert_eq!(ran.code, 0, "{}", ran.err);
  assert_eq!(answer["isError"], true);
  assert!(text(&answer).contains("settings do not read"), "{}", text(&answer));
  assert!(text(&answer).contains(&missing), "{}", text(&answer));
}

// "it serves no resources, and a kind it does not answer ends the call unclean"
#[test]
fn it_serves_no_resources() {
  let (env, _) = env_without_settings("resources");
  let listed = tool(&json!({ "protocol": 1, "kind": "resources_list", "session": {} }).to_string(), &env);

  assert_eq!(listed.answer(), json!({ "resources": [] }));

  let read = tool(
    &json!({ "protocol": 1, "kind": "resources_read", "uri": "voice://servers", "session": {} })
      .to_string(),
    &env,
  );

  assert!(read.answer()["error"].as_str().unwrap().contains("no resources"));

  let claimed = tool(
    &json!({ "protocol": 1, "kind": "claim", "url": "https://example.com", "session": {} }).to_string(),
    &env,
  );

  assert_ne!(claimed.code, 0);
  assert!(claimed.out.is_empty(), "{}", claimed.out);
}
