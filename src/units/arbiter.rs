//! The arbiter's master, as its `docs/api.md` states it (「Demands」): a demand for cards of a service asked, read, and
//! given back. Every waiting line takes its demand from one read of all the master holds, made once a second while any
//! line waits, so no line holds a request of its own open; a demand that read does not hold is read again by its id,
//! since the read may have been taken before it was asked. Which node answers a card is the master's to say.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::watch;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Caller, UnitError};

/// How often the master's demands are read while any line waits on one.
pub const READ_EVERY: Duration = Duration::from_secs(1);

/// The arbiter's master.
#[derive(Clone, Debug)]
pub struct Arbiter {
  caller: Caller,
  reads: Arc<Reads>,
}

/// One read of every demand the master holds, by id; or, where the read failed, whether the master was not reached
/// and why.
#[derive(Clone, Debug)]
pub enum Reading {
  Held(Arc<HashMap<i64, Demand>>),
  Failed { unreached: bool, why: String },
}

/// The reads every waiting line takes its demand from, and whether the loop that makes them stands.
#[derive(Debug)]
struct Reads {
  latest: watch::Sender<Option<Reading>>,
  started: AtomicBool,
}

#[derive(Deserialize)]
struct Page {
  items: Vec<Demand>,
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
  /// The key of the work it continues, as it was asked.
  #[serde(default)]
  pub work_key: Option<String>,
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
    Self {
      caller,
      reads: Arc::new(Reads {
        latest: watch::Sender::new(None),
        started: AtomicBool::new(false),
      }),
    }
  }

  /// Every demand the master holds, read once.
  pub async fn demands(&self) -> Result<Vec<Demand>, UnitError> {
    let request = self.caller.request(Method::GET, &["demands"]);
    let page: Page = self.caller.json(request).await?;

    Ok(page.items)
  }

  /// A waiting line's demand as the next read of the master finds it: the demand, none where the master answers
  /// `404` for it by its id, or the read's failure. A demand the read does not hold is read by its id, since a read
  /// taken before the demand was asked does not hold it. The loop that reads is started by the first line that
  /// waits, and reads only while one does.
  pub async fn watched(&self, id: i64) -> Result<Option<Demand>, UnitError> {
    self.read_while_watched();

    let mut next = self.reads.latest.subscribe();

    next.mark_unchanged();

    if next.changed().await.is_err() {
      return Err(UnitError::Unreached {
        unit: "arbiter",
        why: "the reads of the master stopped".to_owned(),
      });
    }

    let reading = next.borrow().clone();

    match reading {
      Some(Reading::Held(demands)) => match demands.get(&id) {
        Some(demand) => Ok(Some(demand.clone())),
        None => self.demand(id).await,
      },
      Some(Reading::Failed {
        unreached: true,
        why,
      }) => Err(UnitError::Unreached {
        unit: "arbiter",
        why,
      }),
      Some(Reading::Failed {
        unreached: false,
        why,
      }) => Err(UnitError::Refused {
        unit: "arbiter",
        status: 500,
        code: None,
        error: why,
      }),
      None => self.demand(id).await,
    }
  }

  // The one loop that reads the master for every waiting line, started once and reading only while a line waits.
  fn read_while_watched(&self) {
    if self.reads.started.swap(true, Ordering::SeqCst) {
      return;
    }

    let arbiter = self.clone();

    tokio::spawn(async move {
      loop {
        if arbiter.reads.latest.receiver_count() > 0 {
          let reading = match arbiter.demands().await {
            Ok(demands) => {
              Reading::Held(Arc::new(demands.into_iter().map(|demand| (demand.id, demand)).collect()))
            }
            Err(why) => Reading::Failed {
              unreached: matches!(
                why,
                UnitError::Unreached { .. }
                  | UnitError::Refused {
                    status: 502 | 503,
                    ..
                  }
              ),
              why: why.to_string(),
            },
          };

          arbiter.reads.latest.send_replace(Some(reading));
        }

        tokio::time::sleep(READ_EVERY).await;
      }
    });
  }

  /// A demand asked, answered as it stands: granted or waiting.
  pub async fn ask(&self, ask: &Ask) -> Result<Demand, UnitError> {
    let request = self.caller.request(Method::POST, &["demands"]).json(ask);

    self.caller.json(request).await
  }

  /// A demand read as it stands; none where the master holds no such demand.
  pub async fn demand(&self, id: i64) -> Result<Option<Demand>, UnitError> {
    let request = self.caller.request(Method::GET, &["demands", &id.to_string()]);

    match self.caller.json(request).await {
      Ok(demand) => Ok(Some(demand)),
      Err(UnitError::Refused { status: 404, .. }) => Ok(None),
      Err(why) => Err(why),
    }
  }

  /// A demand given back; one the master no longer holds is given back already.
  pub async fn give_back(&self, id: i64) -> Result<(), UnitError> {
    let request = self.caller.request(Method::DELETE, &["demands", &id.to_string()]);

    match self.caller.send(request).await {
      Ok(_) | Err(UnitError::Refused { status: 404, .. }) => Ok(()),
      Err(why) => Err(why),
    }
  }
}
