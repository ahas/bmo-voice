//! The events table (`docs/store.md`): every event the service publishes, numbered, with the member it concerns.

use sea_orm::Value;

use super::{StoreError, all, one, run};
use crate::wire::Event;

/// How many past events the table keeps for a reader resuming.
pub const WINDOW: i64 = 1_000;

/// One event recorded, numbered past every event before it.
pub async fn record(
  db: &sea_orm::DatabaseConnection,
  kind: &str,
  at: i64,
  data: &str,
) -> Result<i64, StoreError> {
  let row = one(
    db,
    r#"INSERT INTO "voice_events" ("kind", "at", "data") VALUES ($1, $2, $3) RETURNING "id""#,
    vec![kind.into(), at.into(), data.into()],
  )
  .await?
  .ok_or_else(|| StoreError("the event was not written".to_owned()))?;
  let id: i64 = row.try_get("", "id").map_err(StoreError::from)?;

  run(
    db,
    r#"DELETE FROM "voice_events" WHERE "id" <= $1"#,
    vec![Value::BigInt(Some(id - WINDOW))],
  )
  .await?;

  Ok(id)
}

/// The events past `after` whose kinds are wanted, oldest first, at most `limit`.
pub async fn since(
  db: &sea_orm::DatabaseConnection,
  after: i64,
  kinds: Option<&[String]>,
  limit: Option<usize>,
) -> Result<Vec<Event>, StoreError> {
  let kinds_json = kinds
    .filter(|kinds| !kinds.is_empty())
    .map(|kinds| serde_json::Value::from(kinds.to_vec()).to_string());
  let rows = all(
    db,
    r#"SELECT "id", "kind", "at", "data" FROM "voice_events"
       WHERE "id" > $1
         AND ($2::text IS NULL OR "kind" IN (SELECT value FROM jsonb_array_elements_text($2::jsonb)))
       ORDER BY "id" LIMIT $3"#,
    vec![
      after.into(),
      kinds_json.map_or(Value::String(None), |kinds| kinds.into()),
      Value::BigInt(Some(limit.map_or(i64::MAX, |limit| i64::try_from(limit + 1).unwrap_or(i64::MAX)))),
    ],
  )
  .await?;

  rows
    .iter()
    .map(|row| {
      let data: String = row.try_get("", "data")?;

      Ok(Event {
        id: row.try_get("", "id")?,
        kind: row.try_get("", "kind")?,
        at: row.try_get("", "at")?,
        data: serde_json::from_str(&data).map_err(|why| StoreError(format!("an event's data: {why}")))?,
      })
    })
    .collect()
}

/// The oldest event number the table keeps, or none where it keeps none.
pub async fn first(db: &sea_orm::DatabaseConnection) -> Result<Option<i64>, StoreError> {
  one(db, r#"SELECT MIN("id") AS "first" FROM "voice_events""#, vec![])
    .await?
    .map(|row| row.try_get::<Option<i64>>("", "first").map_err(StoreError::from))
    .transpose()
    .map(Option::flatten)
}
