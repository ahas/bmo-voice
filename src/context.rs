//! What every part of the voice stands on: its settings, its database, the units it calls, what each waiting
//! synthesis waits on, and the signal each line this process speaks is stopped by.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use sea_orm::DatabaseConnection;
use tokio_util::sync::CancellationToken;

use crate::settings::Settings;
use crate::store;
use crate::units::Units;

/// The voice's settings, its store, its clients of the other units, what its waiting syntheses wait on, and the
/// stop signals of the lines it speaks.
#[derive(Clone, Debug)]
pub struct Context {
  pub settings: Settings,
  pub db: DatabaseConnection,
  pub units: Units,
  pub waits: Waits,
  pub stops: Stops,
}

/// What each synthesis of this process that waits for its card waits on, by its id: held in memory alone, since only
/// the process that waits knows it.
#[derive(Clone, Debug, Default)]
pub struct Waits(Arc<Mutex<HashMap<i64, String>>>);

impl Waits {
  /// What the synthesis waits on now, or that it waits on nothing.
  pub fn set(&self, synthesis: i64, why: Option<&str>) {
    let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);

    match why {
      Some(why) => held.insert(synthesis, why.to_owned()),
      None => held.remove(&synthesis),
    };
  }

  /// What the synthesis waits on, where it waits.
  pub fn of(&self, synthesis: i64) -> Option<String> {
    self.0.lock().unwrap_or_else(PoisonError::into_inner).get(&synthesis).cloned()
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
      stops: Stops::default(),
    })
  }
}
