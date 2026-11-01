//! The card a line is spoken on (`docs/boundary-api.md`, 「The voice servers and the card」): a demand for one card
//! of the voice's service asked of the arbiter's master and waited on however long its grant takes, each reason it
//! waits on told as it changes; read again where the card was held from under the line; and given back as the line
//! ends. A line holds one demand at a time and forgets it only where the master answers `404` for it by its id or
//! answers its give-back. Every demand asked stands in the ledger, one file under `voice/demands/` named by its id,
//! until then, so one a process left behind is given back as the next one starts, with every demand of the voice's
//! work the master holds.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::units::UnitError;
use crate::units::arbiter::{Arbiter, Ask, Demand};

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

// A demand asked and kept in the ledger, asked again while the master is not reached. An ask whose answer did not
// arrive may have been taken in, so before it is asked again the master's demands are read for one asked with the
// line's work key.
async fn asked(
  arbiter: &Arbiter,
  ledger: &Path,
  ask: &Ask,
  said: &mut Option<String>,
  waiting: &mut (dyn FnMut(&str) + Send),
) -> Result<Demand, String> {
  let mut unanswered = false;

  loop {
    if unanswered {
      match taken_in(arbiter, ledger, ask).await {
        Ok(Some(demand)) => return Ok(demand),
        Ok(None) => {}
        Err(why) if !unreached(&why) => {
          return Err(format!("the arbiter's master refused to read its demands: {why}"));
        }
        Err(_) => {
          tell(said, UNREACHED.to_owned(), waiting);
          tokio::time::sleep(AGAIN).await;

          continue;
        }
      }
    }

    match arbiter.ask(ask).await {
      Ok(demand) => {
        keep(ledger, demand.id);

        return Ok(demand);
      }
      Err(why) if !unreached(&why) => {
        return Err(format!("the arbiter's master refused a demand for {}: {why}", ask.service_name));
      }
      Err(_) => {
        unanswered = true;
        tell(said, UNREACHED.to_owned(), waiting);
        tokio::time::sleep(AGAIN).await;
      }
    }
  }
}

// The demand of the line's work key the master took in from an ask whose answer did not arrive, kept in the ledger;
// any beyond the first given back, so the line holds one. None where the master holds none, or the ask names no key.
async fn taken_in(arbiter: &Arbiter, ledger: &Path, ask: &Ask) -> Result<Option<Demand>, UnitError> {
  let Some(key) = &ask.work_key else {
    return Ok(None);
  };
  let mut found = arbiter
    .demands()
    .await?
    .into_iter()
    .filter(|demand| demand.work_key.as_ref() == Some(key));
  let Some(first) = found.next() else {
    return Ok(None);
  };

  keep(ledger, first.id);

  for beyond in found {
    give_back(arbiter, ledger, beyond.id).await;
  }

  Ok(Some(first))
}

// A demand read until a card is granted it, however long that takes. A demand the master answers `404` for, a
// master restarted, is asked again; a refusal of the master's gives the demand back before it ends the wait.
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

    match arbiter.watched(demand.id).await {
      Ok(Some(now)) => demand = now,
      Ok(None) => {
        forget(ledger, demand.id);
        demand = asked(arbiter, ledger, ask, said, waiting).await?;
      }
      Err(why) if !unreached(&why) => {
        give_back(arbiter, ledger, demand.id).await;

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
/// under the line, or a lower tier took it. A refusal of the master's gives the demand back before it ends the wait.
pub async fn again(
  arbiter: &Arbiter,
  ledger: &Path,
  held: &Held,
  ask: &Ask,
  waiting: &mut (dyn FnMut(&str) + Send),
) -> Result<Held, String> {
  let mut said = None;
  let demand = loop {
    match arbiter.demand(held.demand).await {
      Ok(Some(demand)) => break demand,
      Ok(None) => {
        forget(ledger, held.demand);
        break asked(arbiter, ledger, ask, &mut said, waiting).await?;
      }
      Err(why) if !unreached(&why) => {
        give_back(arbiter, ledger, held.demand).await;

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

/// Every demand an earlier process left given back once: each the ledger holds, and each the master holds whose work
/// key starts with `work`, since none of this process's lines holds one yet. It runs before any line asks. One the
/// master refuses to give back stays in the ledger for the next start. Answers how many were given back.
pub async fn give_back_left(arbiter: &Arbiter, ledger: &Path, work: &str) -> usize {
  let mut left: BTreeSet<i64> = std::fs::read_dir(ledger)
    .map(|entries| {
      entries
        .filter_map(Result::ok)
        .filter_map(|one| one.file_name().to_str()?.parse::<i64>().ok())
        .collect()
    })
    .unwrap_or_default();

  match arbiter.demands().await {
    Ok(demands) => left.extend(
      demands
        .into_iter()
        .filter(|demand| demand.work_key.as_deref().is_some_and(|key| key.starts_with(work)))
        .map(|demand| demand.id),
    ),
    Err(why) => {
      eprintln!("voice: the master's demands were not read for those an earlier process left: {why}")
    }
  }

  let mut given = 0;

  for demand in left {
    match arbiter.give_back(demand).await {
      Ok(()) => {
        forget(ledger, demand);
        given += 1;
      }
      Err(why) => eprintln!("voice: the demand {demand} an earlier process left was not given back: {why}"),
    }
  }

  given
}
