//! Agency, as its `docs/api.md` states it (「pool, seat-requests, reservations and roles」, 「sessions」): a seat of
//! its co-worker pool asked under a role, let in, bound to a thrown-away session and given back, and the session's
//! one turn. No call carries a clock: a seat waits in its line and a turn runs for as long as agency takes.

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Caller, UnitError};

/// Agency.
#[derive(Clone, Debug)]
pub struct Agency {
  caller: Caller,
}

/// A seat asked of the co-worker pool.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatAsk {
  pub request_key: String,
  pub owner: String,
  pub role: String,
  pub selection: String,
  pub work_key: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub provider: Option<String>,
}

/// A seat as agency answers it: its token, its state, and the road its work key resolved to.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Seat {
  pub token: String,
  pub state: String,
  pub provider: String,
  pub model: String,
  pub effort: Option<String>,
  pub tier: String,
}

impl Seat {
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
  request: Option<Seat>,
}

#[derive(Deserialize)]
struct Started {
  id: String,
}

impl Agency {
  pub fn new(caller: Caller) -> Self {
    Self { caller }
  }

  /// A seat asked, answered as it stands.
  pub async fn ask_seat(&self, ask: &SeatAsk) -> Result<Seat, UnitError> {
    let request = self.caller.request(Method::POST, &["seat-requests"]).json(ask);

    self.caller.json(request).await
  }

  /// A seat asked for its admission: the seat where the pool let it in, none while its turn has not come.
  pub async fn admit(&self, token: &str) -> Result<Option<Seat>, UnitError> {
    let request = self.caller.request(Method::PATCH, &["seat-requests", token]).json(&json!({}));
    let answered: Admission = self.caller.json(request).await?;

    Ok(answered.request)
  }

  /// A seat bound to the session it launched.
  pub async fn bind(&self, token: &str, session: &str) -> Result<(), UnitError> {
    let change = json!({ "state": "bound", "childId": session });
    let request = self.caller.request(Method::PATCH, &["seat-requests", token]).json(&change);

    self.caller.send(request).await.map(|_| ())
  }

  /// A seat given back with what its work came to, cancelled where no turn ran on it.
  pub async fn give_back(&self, token: &str, outcome: &str, cancel: bool) -> Result<(), UnitError> {
    let mut change = json!({ "state": "finished", "outcome": outcome });

    if cancel {
      change["cancel"] = json!(true);
    }

    let request = self.caller.request(Method::PATCH, &["seat-requests", token]).json(&change);

    self.caller.send(request).await.map(|_| ())
  }

  /// A thrown-away session started: its id.
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
