//! What the tool's tests stand on: the executable handed one request in an environment of the test's own, and
//! its answer read as the tool host reads it.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// A call of one entry, as the tool host hands it.
pub fn call_entry(entry: &str, arguments: Value) -> Value {
  json!({
    "protocol": 1,
    "kind": "call",
    "entry": entry,
    "arguments": arguments,
    "session": { "holderName": "producer", "capabilities": ["room.tools", "room.send"], "variables": {} },
    "callback": "/run/calls/1.sock",
  })
}

/// A call of the voice servers entry, as the tool host hands it.
pub fn call(arguments: Value) -> Value {
  call_entry("voice/servers", arguments)
}

/// How a run of the executable ended.
pub struct Ran {
  pub code: i32,
  pub out: String,
  pub err: String,
}

impl Ran {
  /// Its answer, read as the tool host reads it.
  pub fn answer(&self) -> Value {
    serde_json::from_str(&self.out).unwrap_or_else(|why| panic!("{why}: {} {}", self.out, self.err))
  }
}

/// The executable handed `request` on standard input in the environment given and nothing else.
pub fn tool(request: &str, env: &HashMap<String, String>) -> Ran {
  let mut started = Command::new(env!("CARGO_BIN_EXE_voice-servers-tool"))
    .env_clear()
    .envs(env)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("the tool starts");

  started.stdin.take().unwrap().write_all(request.as_bytes()).unwrap();

  let done = started.wait_with_output().expect("the tool ends");

  Ran {
    code: done.status.code().unwrap_or(-1),
    out: String::from_utf8_lossy(&done.stdout).into_owned(),
    err: String::from_utf8_lossy(&done.stderr).into_owned(),
  }
}

/// An environment naming a settings file that does not stand, under the tests' own scratch folder, so no call
/// reaches an engine.
pub fn env_without_settings(name: &str) -> (HashMap<String, String>, String) {
  let missing = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
    .join(format!("voice-servers-tool-{name}"))
    .join("settings.toml")
    .to_string_lossy()
    .into_owned();

  (HashMap::from([("BMO_VOICE_SETTINGS".to_owned(), missing.clone())]), missing)
}

/// The text of an answer's pieces, joined.
pub fn text(answer: &Value) -> String {
  answer["content"]
    .as_array()
    .unwrap()
    .iter()
    .map(|piece| piece["text"].as_str().unwrap().to_owned())
    .collect::<Vec<_>>()
    .join("\n")
}
