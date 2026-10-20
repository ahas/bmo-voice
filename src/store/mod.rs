//! The voice's store (`docs/store.md`): its four tables in a database of its own, the migrations that make them,
//! the connection every reader opens, and the reads and writes the voice makes of them.

pub mod encodings;
pub mod events;
pub mod migration;
pub mod syntheses;
pub mod transcriptions;
pub mod voices;

use std::str::FromStr;

use sea_orm::{
  ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, SqlxPostgresConnector, Statement, Value,
};
use sea_orm_migration::MigratorTrait;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

/// Why the store did not answer.
#[derive(Debug)]
pub struct StoreError(pub String);

impl std::fmt::Display for StoreError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(&self.0)
  }
}

impl std::error::Error for StoreError {}

impl From<sea_orm::DbErr> for StoreError {
  fn from(why: sea_orm::DbErr) -> Self {
    Self(format!("the voice's database: {why}"))
  }
}

impl From<sqlx::Error> for StoreError {
  fn from(why: sqlx::Error) -> Self {
    Self(format!("the voice's database: {why}"))
  }
}

/// A connection to the store at the address the settings name, a pool of at most `connections`.
pub async fn connect(address: &str, connections: u32) -> Result<DatabaseConnection, StoreError> {
  let options = PgConnectOptions::from_str(address)?;
  let pool = PgPoolOptions::new().max_connections(connections).connect_with(options).await?;

  Ok(SqlxPostgresConnector::from_sqlx_postgres_pool(pool))
}

/// The store's migrations run to the newest: as the service starts, and before the unit command reads or writes.
pub async fn migrate(db: &DatabaseConnection) -> Result<(), StoreError> {
  migration::Migrator::up(db, None).await.map_err(StoreError::from)
}

fn statement(sql: &str, values: Vec<Value>) -> Statement {
  Statement::from_sql_and_values(DbBackend::Postgres, sql, values)
}

/// The first row a query answers, or none.
pub(crate) async fn one(
  db: &DatabaseConnection,
  sql: &str,
  values: Vec<Value>,
) -> Result<Option<QueryResult>, StoreError> {
  Ok(db.query_one(statement(sql, values)).await?)
}

/// Every row a query answers, in its order.
pub(crate) async fn all(
  db: &DatabaseConnection,
  sql: &str,
  values: Vec<Value>,
) -> Result<Vec<QueryResult>, StoreError> {
  Ok(db.query_all(statement(sql, values)).await?)
}

/// A statement run for what it writes; the rows it touched.
pub(crate) async fn run(db: &DatabaseConnection, sql: &str, values: Vec<Value>) -> Result<u64, StoreError> {
  Ok(db.execute(statement(sql, values)).await?.rows_affected())
}
