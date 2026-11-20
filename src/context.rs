//! What every part of the voice stands on: its settings, its database, the units it calls, what each synthesis that
//! has not ended waits on, when its caller last read it, the clocks that end it, and the signal each line this
//! process speaks is stopped by.

use std::collections::HashMap;
use std::fmt;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use sea_orm::DatabaseConnection;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::policy::POLICY;
use crate::settings::Settings;
use crate::store;
use crate::units::Units;
use crate::wire::{SynthesisWait, WaitOn};

/// The voice's settings, its store, its clients of the other units, what its syntheses wait on, when their callers
/// last read them, the clocks that end them, and the stop signals of the lines it speaks.
#[derive(Clone, Debug)]
pub struct Context {
  pub settings: Settings,
  pub db: DatabaseConnection,
  pub units: Units,
  pub waits: Waits,
  pub reads: Reads,
  pub clocks: Clocks,
  pub stops: Stops,
}

/// What a synthesis waits on as its speaking tells it: its kind, the node and the demand where they stand, and an
/// English sentence saying it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wait {
  pub on: WaitOn,
  pub node: Option<String>,
  pub demand: Option<i64>,
  pub said: String,
}

impl Wait {
  /// A wait on the spoken words' derivation.
  pub fn words(said: &str) -> Self {
    Self {
      on: WaitOn::Words,
      node: None,
      demand: None,
      said: said.to_owned(),
    }
  }

  /// A wait on its card, before the master first answers for it.
  pub fn asking() -> Self {
    Self {
      on: WaitOn::Demand,
      node: None,
      demand: None,
      said: "its card is being asked of the arbiter's master".to_owned(),
    }
  }

  /// Where a synthesis stood while it waited so, as the journal line of its end names it: the node and the demand
  /// once its line is on a node, and otherwise what it waited for.
  pub fn stood(&self) -> String {
    match (&self.node, self.demand) {
      (Some(node), Some(demand)) => format!("on {node} (demand {demand})"),
      (Some(node), None) => format!("on {node}"),
      (None, _) => match self.on {
        WaitOn::Words => "waiting on its spoken words",
        WaitOn::Demand => "waiting for its card",
        WaitOn::Node => "waiting on its node",
      }
      .to_owned(),
    }
  }
}

/// What each synthesis of this process that has not ended waits on, by its id, and since when: held in memory alone,
/// since only the process that waits knows it.
#[derive(Clone, Debug, Default)]
pub struct Waits(Arc<Mutex<HashMap<i64, (Wait, i64)>>>);

impl Waits {
  /// What the synthesis waits on now; its moment kept where the wait is the one it held, and now otherwise.
  pub fn set(&self, synthesis: i64, wait: Wait) {
    let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
    let since = match held.get(&synthesis) {
      Some((was, since)) if *was == wait => *since,
      _ => crate::now_ms(),
    };

    held.insert(synthesis, (wait, since));
  }

  /// The synthesis waits on nothing any more.
  pub fn clear(&self, synthesis: i64) {
    self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(&synthesis);
  }

  /// Where the synthesis stands while it waits (`Wait::stood`); none once it waits on nothing.
  pub fn stood(&self, synthesis: i64) -> Option<String> {
    let held = self.0.lock().unwrap_or_else(PoisonError::into_inner);

    held.get(&synthesis).map(|(wait, _)| wait.stood())
  }

  /// What the synthesis waits on as the wire answers it, and the sentence saying it, where it waits.
  pub fn of(&self, synthesis: i64) -> Option<(SynthesisWait, String)> {
    let held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
    let (wait, since) = held.get(&synthesis)?;

    Some((
      SynthesisWait {
        on: wait.on,
        node: wait.node.clone(),
        demand_id: wait.demand,
        since: *since,
      },
      wait.said.clone(),
    ))
  }
}

/// When each synthesis's caller last read it, by its id, for the caller's clock: held in memory alone, since a
/// process that takes a synthesis up starts its caller's clock again.
#[derive(Clone, Debug, Default)]
pub struct Reads(Arc<Mutex<HashMap<i64, Instant>>>);

impl Reads {
  /// The synthesis read by its caller at `at`, as the synthesis clocks count it.
  pub fn read(&self, synthesis: i64, at: Instant) {
    self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(synthesis, at);
  }

  /// When its caller last read it, where it was read.
  pub fn last(&self, synthesis: i64) -> Option<Instant> {
    self.0.lock().unwrap_or_else(PoisonError::into_inner).get(&synthesis).copied()
  }

  /// The synthesis ended: its reads are forgotten.
  pub fn forget(&self, synthesis: i64) {
    self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(&synthesis);
  }
}

/// The time the clocks that end a synthesis are counted on: the runtime's own, or one a test moves by hand, so a
/// test of a clock never waits on the real one.
pub trait Clock: fmt::Debug + Send + Sync {
  /// This moment.
  fn now(&self) -> Instant;

  /// Ends once this clock reaches `due`.
  fn sleep_until(&self, due: Instant) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// The runtime's own clock, the one the service runs on.
#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeClock;

impl Clock for RuntimeClock {
  fn now(&self) -> Instant {
    Instant::now()
  }

  fn sleep_until(&self, due: Instant) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(tokio::time::sleep_until(due))
  }
}

/// The clocks that end a synthesis (`docs/boundary-api.md`, 「The clocks」): their spans, the policy's or shorter
/// where a test sets them, and the clock they are counted on.
#[derive(Clone, Debug)]
pub struct Clocks {
  /// How long a synthesis its caller has not read waits before it fails.
  pub caller_gone: Duration,
  /// How long the node service may take to say how a line's node is reached.
  pub node_service: Duration,
  /// How long one command on a line's node may take.
  pub node_answer: Duration,
  /// How long a line on its node may stand still.
  pub node_still: Duration,
  /// How the tries of a line's derivation are paced, and how many fail before the line does.
  pub spoken: crate::speech::spoken::Pacing,
  /// The clock every span is counted on.
  pub clock: Arc<dyn Clock>,
}

impl Clocks {
  /// This moment, as the spans are counted.
  pub fn now(&self) -> Instant {
    self.clock.now()
  }

  /// Ends once the clock reaches `due`.
  pub async fn until(&self, due: Instant) {
    self.clock.sleep_until(due).await;
  }

  /// What `work` comes to, or nothing where the clock runs `span` first, counted from the first poll.
  pub async fn within<T>(&self, span: Duration, work: impl Future<Output = T>) -> Option<T> {
    let due = self.now() + span;

    tokio::select! {
      done = work => Some(done),
      () = self.until(due) => None,
    }
  }
}

impl Default for Clocks {
  fn default() -> Self {
    let spans = POLICY.clocks;

    Self {
      caller_gone: Duration::from_secs(spans.caller_gone_seconds),
      node_service: Duration::from_secs(spans.node_answer_seconds),
      node_answer: Duration::from_secs(spans.node_answer_seconds),
      node_still: Duration::from_secs(spans.node_still_seconds),
      spoken: crate::speech::spoken::Pacing {
        first_wait: Duration::from_secs(spans.spoken_first_wait_seconds),
        longest_wait: Duration::from_secs(spans.spoken_longest_wait_seconds),
        tries: spans.spoken_tries,
      },
      clock: Arc::new(RuntimeClock),
    }
  }
}

/// The signal each line of this process is stopped by, by its synthesis's id: held in memory alone, since only the
/// process that speaks a line can end its speaking. A line's signal stands from before its speaking starts until it
/// ends, so a stop that comes before the speaking is first polled is heard all the same.
#[derive(Clone, Debug, Default)]
pub struct Stops(Arc<Mutex<HashMap<i64, CancellationToken>>>);

impl Stops {
  /// The signal a line about to be spoken listens on.
  pub fn watch(&self, synthesis: i64) -> CancellationToken {
    let signal = CancellationToken::new();

    self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(synthesis, signal.clone());

    signal
  }

  /// The line's signal let go as its speaking ends.
  pub fn forget(&self, synthesis: i64) {
    self.0.lock().unwrap_or_else(PoisonError::into_inner).remove(&synthesis);
  }

  /// The line told to stop, where this process speaks it.
  pub fn stop(&self, synthesis: i64) {
    if let Some(signal) = self.0.lock().unwrap_or_else(PoisonError::into_inner).get(&synthesis) {
      signal.cancel();
    }
  }
}

impl Context {
  /// The context the settings where they stand name, its store migrated to the newest.
  pub async fn open(connections: u32) -> Result<Self, String> {
    let settings = Settings::load().map_err(|why| why.to_string())?;
    let db = store::connect(&settings.database, connections).await.map_err(|why| why.to_string())?;

    store::migrate(&db).await.map_err(|why| why.to_string())?;
    store::voices::seed_names(&db, &store::voices::seeds(), crate::now_ms() / 1000)
      .await
      .map_err(|why| why.to_string())?;

    let units = Units::new(&settings).map_err(|why| why.to_string())?;

    Ok(Self {
      settings,
      db,
      units,
      waits: Waits::default(),
      reads: Reads::default(),
      clocks: Clocks::default(),
      stops: Stops::default(),
    })
  }
}
