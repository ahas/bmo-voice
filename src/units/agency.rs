//! bmo-agency, as its `docs/api.md` states it (「Agents」, 「Run log and run records」, 「Workers」): the agents'
//! registrations, the run ledger, a run's clock, and a note in a run's log.

use std::collections::BTreeMap;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{Caller, UnitError};

/// The agency.
#[derive(Clone, Debug)]
pub struct Agency {
  caller: Caller,
}

/// One agent of the store, as the agent list names it, with its row's fields.
#[derive(Clone, Debug, Deserialize)]
pub struct Agent {
  pub name: String,
  #[serde(default)]
  pub row: Map<String, Value>,
}

impl Agent {
  /// The voice the agent's registration names, or none.
  pub fn voice(&self) -> Option<String> {
    self.row.get("voice").and_then(Value::as_str).map(str::to_owned)
  }
}

#[derive(Deserialize)]
struct Agents {
  agents: Vec<Agent>,
}

/// A run of the ledger opened: the agent, the room, the unit it runs as, how deep it stands, its cap, when it
/// started and what started it. Times are epoch seconds.
#[derive(Clone, Debug, Serialize)]
pub struct Opening {
  pub agent: String,
  pub chat_id: Option<i64>,
  pub unit: String,
  pub depth: i64,
  pub cap_ms: Option<i64>,
  pub started_at: i64,
  pub started_by: Option<String>,
}

#[derive(Deserialize)]
struct Opened {
  id: i64,
}

/// The program a run is: what runs, its words, where, and with what variables.
#[derive(Clone, Debug, Serialize)]
pub struct Program {
  pub program: String,
  pub args: Vec<String>,
  pub cwd: String,
  pub environment: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct Launch<'a> {
  title: &'a str,
  agent: &'a str,
  command: &'a Program,
}

#[derive(Serialize)]
struct Close<'a> {
  finished_at: i64,
  outcome: &'a str,
}

/// A run's clock: when the latest bound under its name runs out, and the latest moment a keep or an open ask
/// stands until, each in epoch milliseconds.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Clock {
  pub cap_ends: Option<i64>,
  pub kept_until: Option<i64>,
}

/// A line of the kind `note` written into a run's log.
#[derive(Clone, Debug, Serialize)]
pub struct Note {
  pub agent: String,
  pub run: String,
  pub chat: Option<i64>,
  pub text: String,
  pub state: Option<String>,
}

impl Agency {
  pub fn new(caller: Caller) -> Self {
    Self { caller }
  }

  /// Every agent of the store.
  pub async fn agents(&self) -> Result<Vec<Agent>, UnitError> {
    let listed: Agents = self.caller.json(self.caller.request(Method::GET, &["agents"])).await?;

    Ok(listed.agents)
  }

  /// One agent by its name, or none where the store holds no such agent.
  pub async fn agent(&self, name: &str) -> Result<Option<Agent>, UnitError> {
    Ok(self.agents().await?.into_iter().find(|agent| agent.name == name))
  }

  /// A run of the ledger opened; its id.
  pub async fn open_run(&self, opening: &Opening) -> Result<i64, UnitError> {
    let opened: Opened =
      self.caller.json(self.caller.request(Method::POST, &["runs"]).json(opening)).await?;

    Ok(opened.id)
  }

  /// A run's program started in its agent's host, under the run's unit.
  pub async fn launch(
    &self,
    run: i64,
    title: &str,
    agent: &str,
    command: &Program,
  ) -> Result<(), UnitError> {
    let body = Launch {
      title,
      agent,
      command,
    };
    let sent = self.caller.request(Method::POST, &["runs", &run.to_string(), "launch"]).json(&body);

    self.caller.send(sent).await.map(drop)
  }

  /// A run closed with how it ended. A run closed already keeps its closing.
  pub async fn close_run(&self, run: i64, finished_at: i64, outcome: &str) -> Result<(), UnitError> {
    let sent = self.caller.request(Method::POST, &["runs", &run.to_string(), "close"]).json(&Close {
      finished_at,
      outcome,
    });

    match self.caller.send(sent).await {
      Ok(_) | Err(UnitError::Refused { status: 409, .. }) => Ok(()),
      Err(why) => Err(why),
    }
  }

  /// The clock of the runs standing under a name.
  pub async fn clock(&self, run: &str) -> Result<Clock, UnitError> {
    self
      .caller
      .json(self.caller.request(Method::GET, &["runs", "clock"]).query(&[("run", run)]))
      .await
  }

  /// A note written into a run's log.
  pub async fn note(&self, note: &Note) -> Result<(), UnitError> {
    self
      .caller
      .send(self.caller.request(Method::POST, &["log", "notes"]).json(note))
      .await
      .map(drop)
  }
}
