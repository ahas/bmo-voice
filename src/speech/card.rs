//! The card a line is spoken on (`docs/boundary-api.md`, 「The voice servers and the card」): a demand for one card
//! of the voice's service asked of the arbiter's master and waited on however long its grant takes, each reason it
//! waits on told as it changes; read again where the card was held from under the line; and given back as the line
//! ends. Every demand asked stands in the ledger, one file under `voice/demands/` named by its id, until it is given
//! back, so one a process left behind is given back as the next one starts.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::units::UnitError;
use crate::units::arbiter::{Arbiter, Ask, DEMAND_WAIT, Demand};

// How long a call that did not reach the master waits before it is made again.
const AGAIN: Duration = Duration::from_secs(1);

/// What a waiting demand is said to wait on where the master was not reached.
pub const UNREACHED: &str = "The arbiter's master is not reached; the demand is asked again.";

/// What a waiting demand is said to wait on where the master names no reason yet.
pub const IN_LINE: &str = "The demand waits in the arbiter's line for a card.";

/// A card held for a line: the demand, and the node of the card granted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
  pub demand: i64,
  pub node: String,
}

// Whether a call's failure is the master not reached, which is asked again, rather than its refusal.
fn unreached(why: &UnitError) -> bool {
  match why {
    UnitError::Unreached { .. } => true,
    UnitError::Refused { status, .. } => *status == 502 || *status == 503,
  }
}

fn entry(ledger: &Path, demand: i64) -> PathBuf {
  ledger.join(demand.to_string())
}

fn keep(ledger: &Path, demand: i64) {
  let written = std::fs::create_dir_all(ledger).and_then(|()| std::fs::write(entry(ledger, demand), ""));

  if let Err(why) = written {
    eprintln!("voice: the demand {demand} was not written into the ledger: {why}");
  }
}

fn forget(ledger: &Path, demand: i64) {
  let _ = std::fs::remove_file(entry(ledger, demand));
}

// Said once for each reason as it changes.
fn tell(said: &mut Option<String>, why: String, waiting: &mut (dyn FnMut(&str) + Send)) {
  if said.as_ref() != Some(&why) {
    waiting(&why);
    *said = Some(why);
  }
}

// A demand asked and kept in the ledger, asked again while the master is not reached.
async fn asked(
  arbiter: &Arbiter,
  ledger: &Path,
  ask: &Ask,
  said: &mut Option<String>,
  waiting: &mut (dyn FnMut(&str) + Send),
) -> Result<Demand, String> {
  loop {
    match arbiter.ask(ask).await {
      Ok(demand) => {
        keep(ledger, demand.id);

        return Ok(demand);
      }
      Err(why) if !unreached(&why) => {
        return Err(format!("the arbiter's master refused a demand for {}: {why}", ask.service_name));
      }
      Err(_) => {
        tell(said, UNREACHED.to_owned(), waiting);
        tokio::time::sleep(AGAIN).await;
      }
    }
  }
}

// A demand read until a card is granted it, however long that takes. A demand the master no longer holds, a master
// restarted, is asked again.
async fn granted(
  arbiter: &Arbiter,
  ledger: &Path,
  mut demand: Demand,
  ask: &Ask,
  said: &mut Option<String>,
  waiting: &mut (dyn FnMut(&str) + Send),
) -> Result<Held, String> {
  loop {
    if let Some(card) = demand.card() {
      return Ok(Held {
        demand: demand.id,
        node: card.node_name.clone(),
      });
    }

    tell(said, demand.waiting_on().unwrap_or_else(|| IN_LINE.to_owned()), waiting);

    match arbiter.demand(demand.id, DEMAND_WAIT).await {
      Ok(Some(now)) => demand = now,
      Ok(None) => {
        forget(ledger, demand.id);
        demand = asked(arbiter, ledger, ask, said, waiting).await?;
      }
      Err(why) if !unreached(&why) => {
        return Err(format!("the arbiter's master refused to read the demand {}: {why}", demand.id));
      }
      Err(_) => {
        tell(said, UNREACHED.to_owned(), waiting);
        tokio::time::sleep(AGAIN).await;
      }
    }
  }
}

/// A card held for a line: a demand asked and waited on until it is granted, each reason it waits on handed to
/// `waiting` as it changes. Only a refusal of the master's ends it short of a card.
pub async fn hold(
  arbiter: &Arbiter,
  ledger: &Path,
  ask: &Ask,
  waiting: &mut (dyn FnMut(&str) + Send),
) -> Result<Held, String> {
  let mut said = None;
  let demand = asked(arbiter, ledger, ask, &mut said, waiting).await?;

  granted(arbiter, ledger, demand, ask, &mut said, waiting).await
}

/// The card of a demand held already, read again until it is granted again: where other work held the card from
/// under the line, or a lower tier took it.
pub async fn again(
  arbiter: &Arbiter,
  ledger: &Path,
  held: &Held,
  ask: &Ask,
  waiting: &mut (dyn FnMut(&str) + Send),
) -> Result<Held, String> {
  let mut said = None;
  let demand = loop {
    match arbiter.demand(held.demand, Duration::ZERO).await {
      Ok(Some(demand)) => break demand,
      Ok(None) => {
        forget(ledger, held.demand);
        break asked(arbiter, ledger, ask, &mut said, waiting).await?;
      }
      Err(why) if !unreached(&why) => {
        return Err(format!("the arbiter's master refused to read the demand {}: {why}", held.demand));
      }
      Err(_) => {
        tell(&mut said, UNREACHED.to_owned(), waiting);
        tokio::time::sleep(AGAIN).await;
      }
    }
  };

  granted(arbiter, ledger, demand, ask, &mut said, waiting).await
}

/// A demand given back and taken out of the ledger, given back again while the master is not reached; one the
/// master refuses to give back stays in the ledger for the next start.
pub async fn give_back(arbiter: &Arbiter, ledger: &Path, demand: i64) {
  loop {
    match arbiter.give_back(demand).await {
      Ok(()) => {
        forget(ledger, demand);

        return;
      }
      Err(why) if !unreached(&why) => {
        eprintln!("voice: the demand {demand} was not given back: {why}");

        return;
      }
      Err(_) => tokio::time::sleep(AGAIN).await,
    }
  }
}

/// Every demand the ledger holds given back once: what a process left behind as it ended, given back as the next
/// one starts, since none of its lines holds a card yet. Answers how many were given back.
pub async fn give_back_left(arbiter: &Arbiter, ledger: &Path) -> usize {
  let Ok(entries) = std::fs::read_dir(ledger) else {
    return 0;
  };
  let mut given = 0;

  for demand in entries
    .filter_map(Result::ok)
    .filter_map(|one| one.file_name().to_str()?.parse::<i64>().ok())
  {
    match arbiter.give_back(demand).await {
      Ok(()) => {
        forget(ledger, demand);
        given += 1;
      }
      Err(why) => eprintln!("voice: the demand {demand} left in the ledger was not given back: {why}"),
    }
  }

  given
}
