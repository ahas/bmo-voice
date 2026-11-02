//! Agency, as its `docs/api.md` states it (「slots, reservations and places」, 「sessions」): a slot of its
//! co-worker pool asked under a role, let in and given back, and the role's session started on it with its turns.
//! No call carries a clock: a slot waits in its role's line while its owner asks, and a turn runs for as long as
//! agency takes.

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Caller, UnitError};

/// Agency.
#[derive(Clone, Debug)]
pub struct Agency {
  caller: Caller,
}

/// A slot asked of the co-worker pool: on the road its role's order picks, or on the provider and model named
/// together.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotAsk {
  pub request_key: String,
  pub owner: String,
  pub role: String,
  pub source: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub provider: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub model: Option<String>,
}

/// A slot request as agency answers it, of what the engine reads: its token and its state. The road it is let in
/// on is the session's to take.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Slot {
  pub token: String,
  pub state: String,
}

impl Slot {
  /// Whether the pool has let it in.
  pub fn admitted(&self) -> bool {
    self.state == "active"
  }

  /// Whether it was given back before it was let in.
  pub fn ended(&self) -> bool {
    matches!(self.state.as_str(), "released" | "cancelled")
  }
}

#[derive(Deserialize)]
struct Admission {
  request: Option<Slot>,
}

#[derive(Deserialize)]
struct Started {
  id: String,
}

impl Agency {
  pub fn new(caller: Caller) -> Self {
    Self { caller }
  }

  /// A slot asked, answered as it stands.
  pub async fn ask_slot(&self, ask: &SlotAsk) -> Result<Slot, UnitError> {
    let request = self.caller.request(Method::POST, &["slot-requests"]).json(ask);

    self.caller.json(request).await
  }

  /// A slot asked for its admission: the slot where the pool let it in, none while its turn has not come.
  pub async fn admit(&self, token: &str) -> Result<Option<Slot>, UnitError> {
    let request = self.caller.request(Method::PATCH, &["slot-requests", token]).json(&json!({}));
    let answered: Admission = self.caller.json(request).await?;

    Ok(answered.request)
  }

  /// A slot given back with what its work came to, cancelled where no turn ran on it.
  pub async fn give_back(&self, token: &str, outcome: &str, cancel: bool) -> Result<(), UnitError> {
    let mut change = json!({ "state": "finished", "outcome": outcome });

    if cancel {
      change["cancel"] = json!(true);
    }

    let request = self.caller.request(Method::PATCH, &["slot-requests", token]).json(&change);

    self.caller.send(request).await.map(|_| ())
  }

  /// A role's session started on a slot: its id.
  pub async fn start_session(&self, start: &Value) -> Result<String, UnitError> {
    let request = self.caller.request(Method::POST, &["sessions"]).json(start);
    let started: Started = self.caller.json(request).await?;

    Ok(started.id)
  }

  /// One turn sent into a session, answered once it ends: what it wrote, its answer where it gave one, else its
  /// text blocks joined.
  pub async fn turn(&self, session: &str, body: &Value) -> Result<String, UnitError> {
    let request = self.caller.request(Method::POST, &["sessions", session, "turns"]).json(body);
    let turn: Value = self.caller.json(request).await?;

    if let Some(answer) = turn["answer"].as_str().filter(|answer| !answer.trim().is_empty()) {
      return Ok(answer.to_owned());
    }

    let blocks: Vec<&str> = turn["text"]
      .as_array()
      .map(|blocks| blocks.iter().filter_map(Value::as_str).collect())
      .unwrap_or_default();

    Ok(blocks.join("\n"))
  }

  /// A session ended: true where it is gone, false where a turn of it still runs.
  pub async fn end_session(&self, session: &str) -> Result<bool, UnitError> {
    let request = self.caller.request(Method::DELETE, &["sessions", session]);

    match self.caller.send(request).await {
      Ok(_) | Err(UnitError::Refused { status: 404, .. }) => Ok(true),
      Err(UnitError::Refused { status: 409, .. }) => Ok(false),
      Err(why) => Err(why),
    }
  }
}
