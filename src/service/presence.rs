//! The chat agent's presence on the listener stream (`docs/voice-briefing.md`, 「What a listener hears」): one state
//! for the agent as a whole, worked out from the turn events the receiver's stream carries, naming nothing of any
//! room.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{Value, json};
use sse_stream::SseStream;

use super::hub::Hub;
use crate::now_ms;
use crate::units::receiver::Receiver;

// The receiver's events that end a turn.
const TURN_ENDED: [&str; 5] = ["sent", "skipped", "run-failed", "run-stopped", "waited"];

// How long the reader waits before it opens the receiver's stream again.
const RECONNECT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
  Thinking,
  Writing,
}

/// What one turn of each room is doing, collapsed into one state: any turn writing makes it `writing`, otherwise any
/// running turn makes it `thinking`, and no turn at all is `idle`.
#[derive(Default)]
pub struct Tracker {
  turns: BTreeMap<i64, Turn>,
  shown: Option<&'static str>,
  pictured: bool,
}

impl Tracker {
  fn collapsed(&self) -> &'static str {
    if self.turns.values().any(|turn| *turn == Turn::Writing) {
      "writing"
    } else if self.turns.is_empty() {
      "idle"
    } else {
      "thinking"
    }
  }

  // The state as it now stands, said only where it moved.
  fn moved(&mut self, at: i64) -> Option<Value> {
    let state = self.collapsed();

    if self.shown == Some(state) {
      return None;
    }

    self.shown = Some(state);

    Some(json!({ "state": state, "since": at }))
  }

  // The whole picture of what is running replaces what was known: a room no longer answering is dropped, and one
  // still answering keeps the phase its progress last named.
  fn picture(&mut self, live: &Value) -> Option<Value> {
    let answering: Vec<i64> = live["turns"]
      .as_array()
      .into_iter()
      .flatten()
      .filter(|turn| turn.get("speaker").is_none_or(Value::is_null))
      .filter_map(|turn| turn["chatId"].as_i64())
      .collect();

    self.turns.retain(|chat, _| answering.contains(chat));

    for chat in answering {
      self.turns.entry(chat).or_insert(Turn::Thinking);
    }

    self.pictured = true;

    self.moved(live["at"].as_i64().unwrap_or_else(now_ms))
  }

  /// What one frame of the receiver's stream adds: a presence where the state moved, nothing otherwise. Nothing is
  /// said until the picture of what is running has come, which the receiver sends first on every connection.
  pub fn see(&mut self, event: &str, data: &Value) -> Option<Value> {
    if event == "live" {
      return self.picture(data);
    }

    if event != "bot" {
      return None;
    }

    let kind = data["type"].as_str().unwrap_or_default();

    if kind == "live" {
      return if self.pictured { self.picture(&data["live"]) } else { None };
    }

    // A companion's turn names its speaker; it is not the chat agent at work, and its ending is not its own.
    if data.get("speaker").is_some_and(|speaker| !speaker.is_null()) {
      return None;
    }

    let chat = data["chatId"].as_i64()?;

    if kind == "run-started" {
      self.turns.insert(chat, Turn::Thinking);
    } else if kind == "run-progress" && !data["progress"].is_null() {
      let writing = data["progress"]["phase"].as_str() == Some("writing");

      self.turns.insert(chat, if writing { Turn::Writing } else { Turn::Thinking });
    } else if TURN_ENDED.contains(&kind) {
      self.turns.remove(&chat);
    } else {
      return None;
    }

    if self.pictured {
      self.moved(data["at"].as_i64().unwrap_or_else(now_ms))
    } else {
      None
    }
  }
}

/// The receiver's stream read for as long as the service runs, every move of the presence filed on the listener
/// stream. A stream that ends or cannot be opened is opened again after a pause, with a tracker of its own.
pub async fn follow(receiver: Receiver, hub: Arc<Hub>) {
  loop {
    match receiver.events().await {
      Ok(answer) => {
        let mut tracker = Tracker::default();
        let mut events = SseStream::from_bytes_stream(answer.bytes_stream());

        while let Some(Ok(event)) = events.next().await {
          let (Some(name), Some(data)) = (event.event.as_deref(), event.data.as_deref()) else {
            continue;
          };
          let Ok(data) = serde_json::from_str::<Value>(data) else { continue };

          if let Some(presence) = tracker.see(name, &data) {
            hub.file("presence", presence.to_string());
          }
        }
      }
      Err(why) => eprintln!("voice: the receiver's stream could not be opened: {why}"),
    }

    tokio::time::sleep(RECONNECT).await;
  }
}
