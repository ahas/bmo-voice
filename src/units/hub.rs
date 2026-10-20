//! A hub node's command run through its hub, as the node service's target names it (the node service's
//! `docs/api.md`, 「Registry routes」, a node's target): the hub's address, the agent the node is attached as, and
//! the file holding the hub's control token, read for each request and kept nowhere else. The hub runs a command
//! posted to its exec route and streams its output back as server-sent events.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use futures_util::StreamExt;
use serde_json::{Value, json};
use sse_stream::SseStream;

/// The key of the hub's control token in its token file.
const TOKEN_KEY: &str = "JAKE_CONTROL_TOKEN";

/// How much of a command's error stream is kept, in UTF-16 units.
const STDERR_KEPT: usize = 2_000;

/// A hub node as a run reaches it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubTarget {
  pub hub_url: String,
  pub agent: String,
  pub token_file: String,
}

/// How one exec on the node ended: the command's exit with the tail of what it said on its error stream, or what
/// kept it from ending there.
#[derive(Clone, Debug, PartialEq)]
pub enum ExecEnd {
  Exit { code: Option<i64>, stderr: String },
  Error(String),
}

// The token file's path, a leading `~/` read as the account's home.
fn token_path(token_file: &str) -> PathBuf {
  match (token_file.strip_prefix("~/"), std::env::var_os("HOME")) {
    (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
    _ => PathBuf::from(token_file),
  }
}

fn hub_token(target: &HubTarget) -> Result<String, String> {
  let unread = || format!("the token file {} cannot be read", target.token_file);
  let pairs = dotenvy::from_path_iter(token_path(&target.token_file)).map_err(|_| unread())?;
  let mut token = None;

  for pair in pairs {
    let (key, value) = pair.map_err(|_| unread())?;

    if key == TOKEN_KEY {
      token = Some(value);
    }
  }

  token
    .filter(|token| !token.is_empty())
    .ok_or_else(|| format!("the token file {} holds no {TOKEN_KEY}", target.token_file))
}

/// One command on the node through its hub, its stdout's lines handed on as they arrive, and the hub's id for it,
/// which cancelling it takes, written where `started` holds it.
pub async fn hub_exec<F>(
  target: &HubTarget,
  command: &str,
  env: &BTreeMap<String, String>,
  started: Arc<Mutex<Option<String>>>,
  mut on_line: F,
) -> ExecEnd
where
  F: FnMut(&str),
{
  let token = match hub_token(target) {
    Ok(token) => token,
    Err(why) => return ExecEnd::Error(why),
  };
  let asked = reqwest::Client::new()
    .post(format!("{}/exec", target.hub_url.trim_end_matches('/')))
    .query(&[("agent", target.agent.as_str())])
    .bearer_auth(&token)
    .json(&json!({ "shell": "linux", "command": command, "env": env }))
    .send()
    .await;
  let answer = match asked {
    Ok(answer) => answer,
    Err(why) => return ExecEnd::Error(format!("the hub did not answer: {why}")),
  };

  match answer.status().as_u16() {
    200 => {}
    401 | 403 => {
      return ExecEnd::Error(format!("the hub refused the token ({})", answer.status().as_u16()));
    }
    404 | 409 => return ExecEnd::Error(format!("the agent {} is not attached to its hub", target.agent)),
    other => return ExecEnd::Error(format!("the hub answered {other}")),
  }

  let mut events = SseStream::from_bytes_stream(answer.bytes_stream());
  let mut pending: Vec<u8> = Vec::new();
  let mut stderr = String::new();
  let mut end: Option<ExecEnd> = None;

  while let Some(event) = events.next().await {
    let Ok(event) = event else {
      return ExecEnd::Error("the hub's stream broke off".into());
    };
    let Some(body) = event.data.as_deref().filter(|data| !data.is_empty()) else {
      continue;
    };
    let Ok(body) = serde_json::from_str::<Value>(body) else {
      continue;
    };
    let chunk = || {
      body
        .pointer("/chunk/base64")
        .and_then(Value::as_str)
        .and_then(|chunk| STANDARD.decode(chunk).ok())
    };

    match event.event.as_deref() {
      Some("started") => {
        if let Some(id) = body.get("id").and_then(Value::as_str) {
          *started.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(id.to_owned());
        }
      }
      Some("stdout") => {
        if let Some(bytes) = chunk() {
          pending.extend_from_slice(&bytes);

          while let Some(cut) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=cut).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]).into_owned();

            if !line.trim().is_empty() {
              on_line(&line);
            }
          }
        }
      }
      Some("stderr") => {
        if let Some(bytes) = chunk() {
          stderr.push_str(&String::from_utf8_lossy(&bytes));

          let units: Vec<u16> = stderr.encode_utf16().collect();

          if units.len() > STDERR_KEPT {
            stderr = String::from_utf16_lossy(&units[units.len() - STDERR_KEPT..]);
          }
        }
      }
      Some("exit") => {
        end = Some(ExecEnd::Exit {
          code: body.get("code").and_then(Value::as_i64),
          stderr: stderr.clone(),
        });
      }
      Some("error") => {
        end = Some(ExecEnd::Error(
          body
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the hub reported an error")
            .to_owned(),
        ));
      }
      Some("ended") if end.is_none() => {
        end = Some(ExecEnd::Error(format!(
          "the run ended: {}",
          body.get("reason").and_then(Value::as_str).unwrap_or("no reason given")
        )));
      }
      _ => {}
    }
  }

  let rest = String::from_utf8_lossy(&pending).into_owned();

  if !rest.trim().is_empty() {
    on_line(&rest);
  }

  end.unwrap_or_else(|| ExecEnd::Error("the hub's stream closed without the command's end".into()))
}
