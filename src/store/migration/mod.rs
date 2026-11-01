//! The migrations that make the store's tables. The record of what ran is a table of the store's own, so the
//! store may share a database with another service's tables without either's record standing in the other's way.

mod m20261019_000001_tables;
mod m20261020_000001_speech;
mod m20261026_000001_briefings;
mod m20261101_000001_spoken;

use sea_orm_migration::prelude::*;

/// The table the store's migrations keep the record of what ran in.
pub const MIGRATIONS_TABLE: &str = "voice_migrations";

/// Every table the store's migrations make, beside the record of what ran.
pub const OWN_TABLES: [&str; 8] = [
  "voices",
  "voice_names",
  "voice_nodes",
  "voice_briefings",
  "voice_syntheses",
  "voice_transcriptions",
  "voice_encodings",
  "voice_events",
];

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![
      Box::new(m20261019_000001_tables::Migration),
      Box::new(m20261020_000001_speech::Migration),
      Box::new(m20261026_000001_briefings::Migration),
      Box::new(m20261101_000001_spoken::Migration),
    ]
  }

  fn migration_table_name() -> DynIden {
    Alias::new(MIGRATIONS_TABLE).into_iden()
  }
}
