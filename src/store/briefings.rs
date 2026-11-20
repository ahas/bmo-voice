//! The briefings table (`docs/store.md`, 「voice_briefings」): one row for each line spoken for a room, read with the
//! synthesis that spoke it.

use sea_orm::{DatabaseConnection, QueryResult, Value};

use super::{StoreError, all, one, run};
use crate::wire::{Briefing, BriefingKind, BriefingQuery, SynthesisState};

// Every column a briefing is answered from, its synthesis's beside its own.
const COLUMNS: &str = r#"SELECT "b"."run_id", "b"."chat_id", "b"."message_id", "b"."kind", "b"."title",
  "b"."synthesis_id", "b"."plays", "b"."awaits_line", "b"."voice_message_id", "b"."sound_path", "b"."started_at",
  "b"."finished_at", "s"."voice_name", "s"."text", "s"."emotion", "s"."state", "s"."duration_ms"
  FROM "voice_briefings" "b" LEFT JOIN "voice_syntheses" "s" ON "s"."id" = "b"."synthesis_id""#;

/// One row as the table keeps it, with its synthesis's state, and where its sound was kept.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
  pub briefing: Briefing,
  pub sound_path: Option<String>,
}

fn row_of(row: &QueryResult) -> Result<Row, StoreError> {
  let kind: String = row.try_get("", "kind")?;
  let kind = BriefingKind::parse(&kind)
    .ok_or_else(|| StoreError(format!("a briefing is a {kind}, which is no kind")))?;
  let synthesis_id: Option<i64> = row.try_get("", "synthesis_id")?;
  let state: Option<String> = row.try_get("", "state")?;
  // A row the engine did not speak went out before it spoke briefings, and reads as done.
  let state = match (synthesis_id, state) {
    (None, _) => SynthesisState::Done,
    (Some(_), Some(state)) => SynthesisState::parse(&state)
      .ok_or_else(|| StoreError(format!("a synthesis is {state}, which is no state")))?,
    (Some(_), None) => SynthesisState::Failed,
  };
  let plays: i64 = row.try_get("", "plays")?;
  let awaits_line: i64 = row.try_get("", "awaits_line")?;

  Ok(Row {
    briefing: Briefing {
      id: row.try_get("", "run_id")?,
      chat_id: row.try_get("", "chat_id")?,
      message_id: row.try_get("", "message_id")?,
      kind,
      title: row.try_get("", "title")?,
      voice_name: row.try_get("", "voice_name")?,
      text: row.try_get("", "text")?,
      emotion: row.try_get("", "emotion")?,
      synthesis_id,
      state,
      duration_ms: row.try_get("", "duration_ms")?,
      plays: plays != 0,
      awaits_line: awaits_line != 0,
      voice_message_id: row.try_get("", "voice_message_id")?,
      started_at: row.try_get("", "started_at")?,
      finished_at: row.try_get("", "finished_at")?,
    },
    sound_path: row.try_get("", "sound_path")?,
  })
}

/// What a briefing is taken in with.
pub struct Taken<'a> {
  pub chat_id: i64,
  pub message_id: Option<i64>,
  pub kind: BriefingKind,
  pub title: Option<&'a str>,
  pub synthesis_id: i64,
  pub plays: bool,
  pub at: i64,
}

/// A briefing taken in, its synthesis named; one whose kind may await its line and that names none awaits it.
pub async fn insert(db: &DatabaseConnection, taken: &Taken<'_>) -> Result<i64, StoreError> {
  let awaits = taken.kind.awaits_a_line() && taken.message_id.is_none();
  let row = one(
    db,
    r#"INSERT INTO "voice_briefings" ("chat_id", "message_id", "kind", "title", "synthesis_id", "plays",
         "awaits_line", "started_at")
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING "run_id""#,
    vec![
      taken.chat_id.into(),
      Value::BigInt(taken.message_id),
      taken.kind.as_str().into(),
      Value::String(taken.title.map(|title| Box::new(title.to_owned()))),
      taken.synthesis_id.into(),
      i64::from(taken.plays).into(),
      i64::from(awaits).into(),
      taken.at.into(),
    ],
  )
  .await?
  .ok_or_else(|| StoreError("the briefing was not written".to_owned()))?;

  row.try_get("", "run_id").map_err(StoreError::from)
}

/// One briefing, or none where the table holds no such row.
pub async fn get(db: &DatabaseConnection, id: i64) -> Result<Option<Row>, StoreError> {
  one(db, &format!(r#"{COLUMNS} WHERE "b"."run_id" = $1"#), vec![id.into()])
    .await?
    .as_ref()
    .map(row_of)
    .transpose()
}

/// The briefings a query narrows to, newest first, below `after`, at most `limit`.
pub async fn page(
  db: &DatabaseConnection,
  query: &BriefingQuery,
  limit: usize,
) -> Result<Vec<Row>, StoreError> {
  all(
    db,
    &format!(
      r#"{COLUMNS}
       WHERE ($1::bigint IS NULL OR "b"."chat_id" = $1) AND ($2::bigint IS NULL OR "b"."message_id" = $2)
         AND ($3::text IS NULL OR "b"."kind" = $3) AND ($4::bigint IS NULL OR "b"."awaits_line" = $4)
         AND ($5::bigint IS NULL OR "b"."run_id" < $5)
       ORDER BY "b"."run_id" DESC LIMIT $6"#
    ),
    vec![
      Value::BigInt(query.chat_id),
      Value::BigInt(query.message_id),
      Value::String(query.kind.map(|kind| Box::new(kind.as_str().to_owned()))),
      Value::BigInt(query.awaits_line.map(i64::from)),
      Value::BigInt(query.after),
      (limit as i64).into(),
    ],
  )
  .await?
  .iter()
  .map(row_of)
  .collect()
}

/// A briefing's line named, or let go with none; either way it no longer awaits one.
pub async fn set_line(db: &DatabaseConnection, id: i64, message_id: Option<i64>) -> Result<(), StoreError> {
  run(
    db,
    r#"UPDATE "voice_briefings" SET "message_id" = COALESCE($2, "message_id"), "awaits_line" = 0
       WHERE "run_id" = $1"#,
    vec![id.into(), Value::BigInt(message_id)],
  )
  .await?;

  Ok(())
}

/// The briefing a synthesis speaks, by its id, or none where it speaks no briefing.
pub async fn of_synthesis(db: &DatabaseConnection, synthesis_id: i64) -> Result<Option<i64>, StoreError> {
  one(
    db,
    r#"SELECT "run_id" FROM "voice_briefings" WHERE "synthesis_id" = $1 LIMIT 1"#,
    vec![synthesis_id.into()],
  )
  .await?
  .map(|row| row.try_get("", "run_id").map_err(StoreError::from))
  .transpose()
}

/// A briefing's end: when, and where it is done, how long its sound runs and where the sound is kept. Only a briefing
/// not yet ended takes it; answers whether it did, which it does not once it was stopped.
pub async fn ended(
  db: &DatabaseConnection,
  id: i64,
  at: i64,
  done: Option<(f64, &str)>,
) -> Result<bool, StoreError> {
  let taken = run(
    db,
    r#"UPDATE "voice_briefings" SET "finished_at" = $2, "sent_at" = $3, "seconds" = $4, "sound_path" = $5
       WHERE "run_id" = $1 AND "finished_at" IS NULL"#,
    vec![
      id.into(),
      at.into(),
      Value::BigInt(done.map(|_| at)),
      Value::Double(done.map(|(seconds, _)| seconds)),
      Value::String(done.map(|(_, path)| Box::new(path.to_owned()))),
    ],
  )
  .await?;

  Ok(taken > 0)
}

/// One briefing a stop ended, and the synthesis that spoke it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stopped {
  pub briefing: i64,
  pub synthesis: i64,
}

/// Every briefing of a room and a kind, or of every room and every kind where none is named, that has not ended
/// stopped at `at`: one whose synthesis waits, speaks, or is done with its end not yet kept. Each briefing ends and
/// its synthesis is `stopped` in one statement, so a line ending this moment either keeps its own end or is stopped,
/// never both. Answers those it stopped, oldest first.
pub async fn stop(
  db: &DatabaseConnection,
  chat_id: Option<i64>,
  kind: Option<BriefingKind>,
  at: i64,
) -> Result<Vec<Stopped>, StoreError> {
  let rows = all(
    db,
    r#"WITH "ended" AS (
         UPDATE "voice_briefings" AS "b" SET "finished_at" = $2
         FROM "voice_syntheses" AS "s"
         WHERE "s"."id" = "b"."synthesis_id" AND "b"."finished_at" IS NULL
           AND "s"."state" IN ('waiting', 'speaking', 'done')
           AND ($1::bigint IS NULL OR "b"."chat_id" = $1) AND ($4::text IS NULL OR "b"."kind" = $4)
         RETURNING "b"."run_id", "b"."synthesis_id"
       )
       UPDATE "voice_syntheses" AS "s" SET "state" = $3
       FROM "ended" WHERE "s"."id" = "ended"."synthesis_id"
       RETURNING "ended"."run_id", "s"."id""#,
    vec![
      Value::BigInt(chat_id),
      at.into(),
      SynthesisState::Stopped.as_str().into(),
      Value::String(kind.map(|kind| Box::new(kind.as_str().to_owned()))),
    ],
  )
  .await?;
  let mut stopped = rows
    .iter()
    .map(|row| {
      Ok(Stopped {
        briefing: row.try_get("", "run_id")?,
        synthesis: row.try_get("", "id")?,
      })
    })
    .collect::<Result<Vec<_>, StoreError>>()?;

  stopped.sort_by_key(|one| one.briefing);

  Ok(stopped)
}
