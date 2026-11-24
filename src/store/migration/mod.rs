//! The migrations that make the store's tables, from the 1.0.0 baseline (docs/store.md, 「The migrations」).
//! The record of what ran is a table of the store's own, so the store may share a database with another
//! service's tables without either's record standing in the other's way.

mod m20261124_000001_baseline;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};
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

/// The migration that makes the schema whole.
pub const BASELINE: &str = m20261124_000001_baseline::NAME;

/// The record an install from before 1.0.0 holds once every migration of its last release ran. Its schema
/// is the baseline's.
pub const BEFORE: [&str; 9] = [
  "m20261019_000001_tables",
  "m20261020_000001_speech",
  "m20261026_000001_briefings",
  "m20261101_000001_spoken",
  "m20261101_000002_language",
  "m20261102_000001_key",
  "m20261113_000001_failure",
  "m20261114_000001_failure_kept",
  "m20261123_000001_asked_by",
];

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
  fn migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(m20261124_000001_baseline::Migration)]
  }

  fn migration_table_name() -> DynIden {
    Alias::new(MIGRATIONS_TABLE).into_iden()
  }
}

/// The migrations run, the baseline first. A database whose record names every migration of the last
/// release before 1.0.0, and nothing else, has that record rewritten to name the baseline alone, touching no
/// table and no row; one whose record names only part of them is refused.
pub async fn up(db: &DatabaseConnection) -> Result<(), DbErr> {
  adopt(db).await?;
  Migrator::up(db, None).await
}

async fn adopt(db: &DatabaseConnection) -> Result<(), DbErr> {
  let tx = db.begin().await?;
  let statement = |sql: String, values: Vec<sea_orm::Value>| {
    Statement::from_sql_and_values(DbBackend::Postgres, sql, values)
  };
  let recorded = tx
    .query_one(statement(
      format!("SELECT to_regclass('{MIGRATIONS_TABLE}') IS NOT NULL AS held"),
      vec![],
    ))
    .await?
    .map(|row| row.try_get::<bool>("", "held"))
    .transpose()?
    .unwrap_or(false);
  if !recorded {
    return Ok(());
  }
  tx.execute_unprepared(&format!(r#"LOCK TABLE "{MIGRATIONS_TABLE}" IN EXCLUSIVE MODE"#))
    .await?;
  let held = tx
    .query_all(statement(
      format!(r#"SELECT "version" FROM "{MIGRATIONS_TABLE}" ORDER BY "version""#),
      vec![],
    ))
    .await?
    .into_iter()
    .map(|row| row.try_get::<String>("", "version"))
    .collect::<Result<Vec<_>, _>>()?;
  if held.is_empty() || !held.iter().all(|version| BEFORE.contains(&version.as_str())) {
    return Ok(());
  }
  if held.len() != BEFORE.len() {
    let reached = held.last().map_or("", String::as_str);
    return Err(DbErr::Migration(format!(
      "the database holds the migrations of a release before 1.0.0 only through {reached}; that release's \
       last migration runs first"
    )));
  }
  tx.execute_unprepared(&format!(r#"DELETE FROM "{MIGRATIONS_TABLE}""#)).await?;
  tx.execute(statement(
    format!(
      r#"INSERT INTO "{MIGRATIONS_TABLE}" ("version", "applied_at") VALUES ($1, EXTRACT(EPOCH FROM now())::bigint)"#
    ),
    vec![BASELINE.into()],
  ))
  .await?;
  tx.commit().await
}
