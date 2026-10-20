//! The transcriptions table (`docs/store.md`): one row for each set of sound files handed to the transcriber, its
//! state and what was heard.

use sea_orm::{DatabaseConnection, QueryResult, Value};

use super::{StoreError, one, run};
use crate::wire::Transcription;

/// One row as the table keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
  pub id: i64,
  pub language: String,
  pub state: String,
  pub text: Option<String>,
  pub created_at: i64,
}

impl Row {
  /// The row as the wire answers it.
  pub fn transcription(&self) -> Transcription {
    Transcription {
      id: self.id,
      language: self.language.clone(),
      state: self.state.clone(),
      text: self.text.clone(),
      created_at: self.created_at,
    }
  }
}

fn row_of(row: &QueryResult) -> Result<Row, StoreError> {
  Ok(Row {
    id: row.try_get("", "id")?,
    language: row.try_get("", "language")?,
    state: row.try_get("", "state")?,
    text: row.try_get("", "text")?,
    created_at: row.try_get("", "created_at")?,
  })
}

/// A transcription taken in, waiting for its files.
pub async fn insert(db: &DatabaseConnection, language: &str, at: i64) -> Result<i64, StoreError> {
  let row = one(
    db,
    r#"INSERT INTO "voice_transcriptions" ("language", "state", "created_at")
       VALUES ($1, 'waiting', $2) RETURNING "id""#,
    vec![language.into(), at.into()],
  )
  .await?
  .ok_or_else(|| StoreError("the transcription was not written".to_owned()))?;

  row.try_get("", "id").map_err(StoreError::from)
}

/// One transcription, or none where the table holds no such row.
pub async fn get(db: &DatabaseConnection, id: i64) -> Result<Option<Row>, StoreError> {
  one(
    db,
    r#"SELECT "id", "language", "state", "text", "created_at"
       FROM "voice_transcriptions" WHERE "id" = $1"#,
    vec![id.into()],
  )
  .await?
  .as_ref()
  .map(row_of)
  .transpose()
}

/// A transcription's state and what was heard.
pub async fn set_state(
  db: &DatabaseConnection,
  id: i64,
  state: &str,
  text: Option<&str>,
) -> Result<(), StoreError> {
  run(
    db,
    r#"UPDATE "voice_transcriptions" SET "state" = $2, "text" = $3 WHERE "id" = $1"#,
    vec![id.into(), state.into(), text.map_or(Value::String(None), |text| text.into())],
  )
  .await?;

  Ok(())
}
