//! What every part of the voice stands on: its settings, its database, the units it calls, and what each waiting
//! synthesis waits on.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use sea_orm::DatabaseConnection;

use crate::settings::Settings;
use crate::store;
use crate::units::Units;

/// The voice's settings, its store, its clients of the other units, and what its waiting syntheses wait on.
#[derive(Clone, Debug)]
pub struct Context {
  pub settings: Settings,
  pub db: DatabaseConnection,
  pub units: Units,
  pub waits: Waits,
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
    })
  }
}
