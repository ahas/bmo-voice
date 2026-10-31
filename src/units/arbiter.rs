//! The arbiter's master, as its `docs/api.md` states it (「Demands」): a demand for cards of a service asked, read
//! with its answer held open while it waits, and given back. Which node answers a card is the master's to say.

use std::time::Duration;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Caller, UnitError};

/// How long one read of a demand holds its answer open at the master.
pub const DEMAND_WAIT: Duration = Duration::from_secs(30);

// How long a call waits for its answer beyond the time it asks the master to hold it open: a connection that
// answers nothing within it is the master not reached, and the call is made again.
const CALL_SLACK: Duration = Duration::from_secs(15);

/// The arbiter's master.
#[derive(Clone, Debug)]
pub struct Arbiter {
  caller: Caller,
}

/// What a demand asks: the service, how many cards, the key of the work it continues and a reference the master
/// only shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ask {
  pub service_name: String,
  pub count: u32,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub work_key: Option<String>,
  #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
  pub reference: Option<String>,
}

/// One card a demand was granted: its node, and where the instance there answers where its adapter says.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Card {
  pub node_name: String,
  #[serde(default)]
  pub address: Option<String>,
  #[serde(default)]
  pub key_file: Option<String>,
}

/// Why a demand's card waits: a kebab-case code and an English sentence.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Reason {
  pub code: String,
  pub text: String,
}

/// A demand as the master answers it, the parts the voice reads.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Demand {
  pub id: i64,
  #[serde(default)]
  pub cards: Vec<Card>,
  /// `waiting`, `granted`, `partly` or `lent`.
  pub state: String,
  /// The claimant or lower tier that stopped the service on one of its cards.
  #[serde(default)]
  pub taken: Option<Value>,
  #[serde(default)]
  pub reason: Option<Reason>,
}

impl Demand {
  /// The card the demand holds now: granted, with nothing taking it.
  pub fn card(&self) -> Option<&Card> {
    (self.state == "granted" && self.taken.as_ref().is_none_or(Value::is_null))
      .then(|| self.cards.first())
      .flatten()
  }

  /// What the demand waits on, as the master says it; none where it says nothing.
  pub fn waiting_on(&self) -> Option<String> {
    if let Some(reason) = &self.reason {
      return Some(reason.text.clone());
    }

    match &self.taken {
      Some(Value::String(name)) => Some(format!("The card was taken by {name}.")),
      Some(Value::Null) | None => None,
      Some(other) => Some(format!("The card was taken by {other}.")),
    }
  }
}

impl Arbiter {
  pub fn new(caller: Caller) -> Self {
    Self { caller }
  }

  /// A demand asked, answered as it stands: granted or waiting.
  pub async fn ask(&self, ask: &Ask) -> Result<Demand, UnitError> {
    let request = self.caller.request(Method::POST, &["demands"]).json(ask).timeout(CALL_SLACK);

    self.caller.json(request).await
  }

  /// A demand read, its answer held open up to `wait` while it waits; none where the master holds no such demand.
  pub async fn demand(&self, id: i64, wait: Duration) -> Result<Option<Demand>, UnitError> {
    let request = self
      .caller
      .request(Method::GET, &["demands", &id.to_string()])
      .query(&[("waitMs", wait.as_millis())])
      .timeout(wait + CALL_SLACK);

    match self.caller.json(request).await {
      Ok(demand) => Ok(Some(demand)),
      Err(UnitError::Refused { status: 404, .. }) => Ok(None),
      Err(why) => Err(why),
    }
  }

  /// A demand given back; one the master no longer holds is given back already.
  pub async fn give_back(&self, id: i64) -> Result<(), UnitError> {
    let request = self.caller.request(Method::DELETE, &["demands", &id.to_string()]).timeout(CALL_SLACK);

    match self.caller.send(request).await {
      Ok(_) | Err(UnitError::Refused { status: 404, .. }) => Ok(()),
      Err(why) => Err(why),
    }
  }
}
