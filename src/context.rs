//! What every part of the voice stands on: its settings, its database and the units it calls.

use sea_orm::DatabaseConnection;

use crate::settings::Settings;
use crate::store;
use crate::units::Units;

/// The voice's settings, its store and its clients of the other units.
#[derive(Clone, Debug)]
pub struct Context {
  pub settings: Settings,
  pub db: DatabaseConnection,
  pub units: Units,
}

impl Context {
  /// The context the settings where they stand name, its store migrated to the newest.
  pub async fn open(connections: u32) -> Result<Self, String> {
    let settings = Settings::load().map_err(|why| why.to_string())?;
    let db = store::connect(&settings.database, connections).await.map_err(|why| why.to_string())?;

    store::migrate(&db).await.map_err(|why| why.to_string())?;

    let units = Units::new(&settings).map_err(|why| why.to_string())?;

    Ok(Self {
      settings,
      db,
      units,
    })
  }
}
