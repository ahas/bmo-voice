//! A document's narration (`docs/boundary-api.md`, 「Narrating a document」): the wait on its synthesis, bounded and
//! told as it goes. Speaking the text and attaching the sound are the command's, through the voice's own client and
//! the archive's.

use std::future::Future;
use std::time::Duration;

use tokio::time::Instant;

use crate::client::ClientFailure;
use crate::wire::{Synthesis, SynthesisState};

/// How long a narration's synthesis is waited on where its caller names no bound.
pub const NARRATION_WAIT: Duration = Duration::from_secs(60 * 60);

/// How often a synthesis that has not ended is read again.
const LOOK_AGAIN: Duration = Duration::from_millis(500);

/// How often where a synthesis stands is told again while neither its state nor what it waits on changes.
const TELL_AGAIN: Duration = Duration::from_secs(5 * 60);

/// Why a wait on a synthesis ended without its sound.
#[derive(Debug)]
pub enum NotSpoken {
  /// The synthesis failed; the engine's sentence why.
  Failed { id: i64, failure: Option<String> },
  /// Its briefing was stopped.
  Stopped { id: i64 },
  /// The bound ran out while it had not ended: how it stood when it was let go.
  OutOfTime {
    id: i64,
    state: SynthesisState,
    waited: Duration,
    waiting_on: Option<String>,
  },
  /// The voice service refused a read of it, or answered in no shape the route answers.
  Unread { id: i64, why: ClientFailure },
}

impl std::fmt::Display for NotSpoken {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Failed { id, failure } => {
        write!(
          f,
          "synthesis {id} failed: {}",
          failure.as_deref().unwrap_or("the engine gave no reason")
        )
      }
      Self::Stopped { id } => write!(f, "synthesis {id} was stopped"),
      Self::OutOfTime {
        id,
        state,
        waited,
        waiting_on,
      } => write!(
        f,
        "synthesis {id} was still {} after {}, waiting on: {}; it is let go, and the engine ends it once it goes unread",
        state.as_str(),
        span(*waited),
        waiting_on.as_deref().unwrap_or("nothing the engine named"),
      ),
      Self::Unread { id, why } => write!(f, "synthesis {id} could not be read: {why}"),
    }
  }
}

impl std::error::Error for NotSpoken {}

/// A span as minutes and seconds.
fn span(waited: Duration) -> String {
  let seconds = waited.as_secs();

  format!("{}m{:02}s", seconds / 60, seconds % 60)
}

/// Waits on a synthesis taken in until it ends, and at most `within` from now: answers it once `done`, and why
/// otherwise. `read` reads it again; `tell` is handed a line saying where it stands, as it begins, each time its state
/// or what it waits on changes, and again every five minutes while neither does. A read nothing answers is told and
/// asked again within the bound, and the bound holds over a read that does not come back.
pub async fn wait<Read, Reading>(
  taken: Synthesis,
  within: Duration,
  mut read: Read,
  mut tell: impl FnMut(&str),
) -> Result<Synthesis, NotSpoken>
where
  Read: FnMut(i64) -> Reading,
  Reading: Future<Output = Result<Synthesis, ClientFailure>>,
{
  let id = taken.id;
  let began = Instant::now();
  let deadline = began + within;
  let mut held = taken;
  let mut told: Option<(SynthesisState, Option<String>)> = None;
  let mut told_at = began;

  loop {
    match held.state {
      SynthesisState::Done => return Ok(held),
      SynthesisState::Failed => {
        return Err(NotSpoken::Failed {
          id,
          failure: held.failure,
        });
      }
      SynthesisState::Stopped => return Err(NotSpoken::Stopped { id }),
      SynthesisState::Waiting | SynthesisState::Speaking => {}
    }

    let now = Instant::now();
    let standing = (held.state, held.waiting_on.clone());

    if told.as_ref() != Some(&standing) || now - told_at >= TELL_AGAIN {
      tell(&format!(
        "synthesis {id} {} for {}: {}",
        held.state.as_str(),
        span(now - began),
        held.waiting_on.as_deref().unwrap_or("waits on nothing the engine names")
      ));
      told = Some(standing);
      told_at = now;
    }

    let out_of_time = |held: Synthesis| NotSpoken::OutOfTime {
      id,
      state: held.state,
      waited: Instant::now() - began,
      waiting_on: held.waiting_on,
    };

    if now >= deadline {
      return Err(out_of_time(held));
    }

    tokio::time::sleep_until((now + LOOK_AGAIN).min(deadline)).await;

    match tokio::time::timeout_at(deadline, read(id)).await {
      Err(_) => return Err(out_of_time(held)),
      Ok(Ok(read)) => held = read,
      Ok(Err(ClientFailure::Unreached(why))) => {
        tell(&format!("synthesis {id}: the voice service did not answer, asked again: {why}"));
      }
      Ok(Err(why)) => return Err(NotSpoken::Unread { id, why }),
    }
  }
}
