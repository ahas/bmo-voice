//! The syntheses table (`docs/store.md`): one row for each text handed to the engine, and its state.

use sea_orm::{DatabaseConnection, QueryResult, Value};

use super::{StoreError, all, one, run};
use crate::wire::{Synthesis, SynthesisState};

/// One row as the table keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
  pub id: i64,
  pub voice_name: String,
  pub text: String,
  pub spoken: Option<String>,
  pub emotion: Option<String>,
  pub state: SynthesisState,
  pub duration_ms: Option<i64>,
  pub created_at: i64,
}

impl Row {
  /// The row as the wire answers it.
  pub fn synthesis(&self) -> Synthesis {
    Synthesis {
      id: self.id,
      voice_name: self.voice_name.clone(),
      text: self.text.clone(),
      spoken: self.spoken.clone(),
      emotion: self.emotion.clone(),
      state: self.state,
      waiting_on: None,
      duration_ms: self.duration_ms,
      created_at: self.created_at,
    }
  }
}

fn row_of(row: &QueryResult) -> Result<Row, StoreError> {
  let state: String = row.try_get("", "state")?;
  let state = SynthesisState::parse(&state)
    .ok_or_else(|| StoreError(format!("a synthesis is {state}, which is no state")))?;

  Ok(Row {
    id: row.try_get("", "id")?,
    voice_name: row.try_get("", "voice_name")?,
    text: row.try_get("", "text")?,
    spoken: row.try_get("", "spoken")?,
    emotion: row.try_get("", "emotion")?,
    state,
    duration_ms: row.try_get("", "duration_ms")?,
    created_at: row.try_get("", "created_at")?,
  })
}

/// A synthesis taken in, waiting to be spoken: its written words and, where the caller gave them, its spoken words.
pub async fn insert(
  db: &DatabaseConnection,
  voice_name: &str,
  text: &str,
  spoken: Option<&str>,
  emotion: Option<&str>,
  at: i64,
) -> Result<i64, StoreError> {
  let row = one(
    db,
    r#"INSERT INTO "voice_syntheses" ("voice_name", "text", "spoken", "emotion", "state", "created_at")
       VALUES ($1, $2, $3, $4, $5, $6) RETURNING "id""#,
    vec![
      voice_name.into(),
      text.into(),
      spoken.map_or(Value::String(None), |spoken| spoken.into()),
      emotion.map_or(Value::String(None), |emotion| emotion.into()),
      SynthesisState::Waiting.as_str().into(),
      at.into(),
    ],
  )
  .await?
  .ok_or_else(|| StoreError("the synthesis was not written".to_owned()))?;

  row.try_get("", "id").map_err(StoreError::from)
}

/// One synthesis, or none where the table holds no such row.
pub async fn get(db: &DatabaseConnection, id: i64) -> Result<Option<Row>, StoreError> {
  one(
    db,
    r#"SELECT "id", "voice_name", "text", "spoken", "emotion", "state", "duration_ms", "created_at"
       FROM "voice_syntheses" WHERE "id" = $1"#,
    vec![id.into()],
  )
  .await?
  .as_ref()
  .map(row_of)
  .transpose()
}

/// A synthesis's spoken words, once they stand.
pub async fn set_spoken(db: &DatabaseConnection, id: i64, spoken: &str) -> Result<(), StoreError> {
  run(
    db,
    r#"UPDATE "voice_syntheses" SET "spoken" = $2 WHERE "id" = $1"#,
    vec![id.into(), spoken.into()],
  )
  .await?;

  Ok(())
}

/// A synthesis's state, and how long its sound runs where it is done.
pub async fn set_state(
  db: &DatabaseConnection,
  id: i64,
  state: SynthesisState,
  duration_ms: Option<i64>,
) -> Result<(), StoreError> {
  run(
    db,
    r#"UPDATE "voice_syntheses" SET "state" = $2, "duration_ms" = $3 WHERE "id" = $1"#,
    vec![id.into(), state.as_str().into(), Value::BigInt(duration_ms)],
  )
  .await?;

  Ok(())
}

/// A voice's syntheses, newest first, past `after`.
pub async fn of_voice(
  db: &DatabaseConnection,
  voice_name: &str,
  after: Option<i64>,
  limit: usize,
) -> Result<Vec<Row>, StoreError> {
  all(
    db,
    r#"SELECT "id", "voice_name", "text", "spoken", "emotion", "state", "duration_ms", "created_at"
       FROM "voice_syntheses"
       WHERE "voice_name" = $1 AND ($2 = 0 OR "id" < $2)
       ORDER BY "id" DESC LIMIT $3"#,
    vec![voice_name.into(), after.unwrap_or(0).into(), (limit as i64).into()],
  )
  .await?
  .iter()
  .map(row_of)
  .collect()
}
