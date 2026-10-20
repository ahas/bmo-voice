//! The encodings table (`docs/store.md`): one row for each reference recording encoded into a sound, and where that
//! sound stands.

use sea_orm::{DatabaseConnection, QueryResult, Value};

use super::{StoreError, one, run};
use crate::wire::Encoding;

/// One row as the table keeps it.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
  pub id: i64,
  pub state: String,
  pub sound_path: Option<String>,
  pub created_at: i64,
}

impl Row {
  /// The row as the wire answers it.
  pub fn encoding(&self) -> Encoding {
    Encoding {
      id: self.id,
      state: self.state.clone(),
      created_at: self.created_at,
    }
  }
}

fn row_of(row: &QueryResult) -> Result<Row, StoreError> {
  Ok(Row {
    id: row.try_get("", "id")?,
    state: row.try_get("", "state")?,
    sound_path: row.try_get("", "sound_path")?,
    created_at: row.try_get("", "created_at")?,
  })
}

/// An encoding taken in.
pub async fn insert(db: &DatabaseConnection, from_wav: &str, at: i64) -> Result<i64, StoreError> {
  let row = one(
    db,
    r#"INSERT INTO "voice_encodings" ("state", "from_wav", "created_at")
       VALUES ('waiting', $1, $2) RETURNING "id""#,
    vec![from_wav.into(), at.into()],
  )
  .await?
  .ok_or_else(|| StoreError("the encoding was not written".to_owned()))?;

  row.try_get("", "id").map_err(StoreError::from)
}

/// An encoding's state.
pub async fn set_state(
  db: &DatabaseConnection,
  id: i64,
  state: &str,
  sound_path: Option<&str>,
) -> Result<(), StoreError> {
  run(
    db,
    r#"UPDATE "voice_encodings" SET "state" = $2, "sound_path" = $3 WHERE "id" = $1"#,
    vec![id.into(), state.into(), sound_path.map_or(Value::String(None), |path| path.into())],
  )
  .await?;

  Ok(())
}

/// One encoding, or none where the table holds no such row.
pub async fn get(db: &DatabaseConnection, id: i64) -> Result<Option<Row>, StoreError> {
  one(
    db,
    r#"SELECT "id", "state", "sound_path", "created_at" FROM "voice_encodings" WHERE "id" = $1"#,
    vec![id.into()],
  )
  .await?
  .as_ref()
  .map(row_of)
  .transpose()
}
