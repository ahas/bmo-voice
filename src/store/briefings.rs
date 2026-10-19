//! The row a briefing keeps as it goes (`docs/store.md`, 「voice_briefings」; `docs/voice-briefing.md`, 「Where
//! each voice is written down」). A briefing's start and end stand on its own row, so nothing here reads another
//! unit's run ledger.

use sea_orm::{DatabaseConnection, QueryResult};

use super::{StoreError, all, one, run};

/// The kind a summary is kept as, which the rules of one summary at a time in a room read.
pub const SUMMARY: &str = "summary";

/// The kind a line's full reading is kept as.
pub const FULL: &str = "full";

/// A briefing's row as it is opened.
#[derive(Clone, Debug, PartialEq)]
pub struct Opening<'a> {
  pub run_id: i64,
  pub chat_id: i64,
  pub message_id: Option<i64>,
  pub kind: &'a str,
  pub title: Option<&'a str>,
  pub awaits_line: bool,
  pub started_at: i64,
}

/// A briefing's row as a room's briefings list it.
#[derive(Clone, Debug, PartialEq)]
pub struct Briefed {
  pub run_id: i64,
  pub chat_id: i64,
  pub message_id: Option<i64>,
  pub voice_message_id: Option<i64>,
  pub kind: String,
  pub title: Option<String>,
  pub awaits_line: bool,
  pub started_at: Option<i64>,
  pub finished: bool,
  pub sound_path: Option<String>,
}

fn briefed(row: &QueryResult) -> Result<Briefed, StoreError> {
  Ok(Briefed {
    run_id: row.try_get("", "run_id")?,
    chat_id: row.try_get("", "chat_id")?,
    message_id: row.try_get("", "message_id")?,
    voice_message_id: row.try_get("", "voice_message_id")?,
    kind: row.try_get("", "kind")?,
    title: row.try_get("", "title")?,
    awaits_line: row.try_get::<i64>("", "awaits_line")? == 1,
    started_at: row.try_get("", "started_at")?,
    finished: row.try_get::<Option<i64>>("", "finished_at")?.is_some(),
    sound_path: row.try_get("", "sound_path")?,
  })
}

const BRIEFED: &str = r#"SELECT "run_id", "chat_id", "message_id", "voice_message_id", "kind", "title", "awaits_line",
  "started_at", "finished_at", "sound_path" FROM "voice_briefings""#;

/// A briefing's row opened, or opened again for a run that kept one: a summary awaits its line until one is named,
/// and a full reading stands on its line.
pub async fn open(db: &DatabaseConnection, opening: &Opening<'_>) -> Result<(), StoreError> {
  run(
    db,
    r#"INSERT INTO "voice_briefings" ("run_id", "chat_id", "message_id", "awaits_line", "kind", "title", "started_at")
       VALUES ($1, $2, $3, $4, $5, $6, $7)
       ON CONFLICT ("run_id") DO UPDATE SET "chat_id" = excluded."chat_id", "message_id" = excluded."message_id",
         "awaits_line" = excluded."awaits_line", "kind" = excluded."kind", "title" = excluded."title",
         "started_at" = excluded."started_at", "finished_at" = NULL,
         "voice_message_id" = NULL, "sent_at" = NULL, "seconds" = NULL, "sound_path" = NULL"#,
    vec![
      opening.run_id.into(),
      opening.chat_id.into(),
      opening.message_id.into(),
      i64::from(opening.awaits_line).into(),
      opening.kind.into(),
      opening.title.map(str::to_owned).into(),
      opening.started_at.into(),
    ],
  )
  .await
  .map(drop)
}

/// A room's briefings, in the order they were launched; only the summaries still awaiting their line where
/// `awaiting`.
pub async fn of_room(
  db: &DatabaseConnection,
  chat_id: i64,
  awaiting: bool,
) -> Result<Vec<Briefed>, StoreError> {
  let sql = if awaiting {
    format!(r#"{BRIEFED} WHERE "chat_id" = $1 AND "awaits_line" = 1 ORDER BY "run_id""#)
  } else {
    format!(r#"{BRIEFED} WHERE "chat_id" = $1 ORDER BY "run_id""#)
  };

  all(db, &sql, vec![chat_id.into()]).await?.iter().map(briefed).collect()
}

/// One briefing's row, or none for a run that belongs to no line.
pub async fn of_run(db: &DatabaseConnection, run_id: i64) -> Result<Option<Briefed>, StoreError> {
  one(db, &format!(r#"{BRIEFED} WHERE "run_id" = $1"#), vec![run_id.into()])
    .await?
    .as_ref()
    .map(briefed)
    .transpose()
}

/// The full reading standing on a line, the newest first, or none.
pub async fn reading_of_line(
  db: &DatabaseConnection,
  chat_id: i64,
  message_id: i64,
) -> Result<Option<Briefed>, StoreError> {
  one(
    db,
    &format!(r#"{BRIEFED} WHERE "chat_id" = $1 AND "message_id" = $2 AND "kind" = 'full' ORDER BY "run_id" DESC LIMIT 1"#),
    vec![chat_id.into(), message_id.into()],
  )
  .await?
  .as_ref()
  .map(briefed)
  .transpose()
}

/// The line a summary's round sent, named under it; it no longer awaits one. Answers whether the run kept a row.
pub async fn tie_line(db: &DatabaseConnection, run_id: i64, message_id: i64) -> Result<bool, StoreError> {
  Ok(
    run(
      db,
      r#"UPDATE "voice_briefings" SET "message_id" = $1, "awaits_line" = 0 WHERE "run_id" = $2"#,
      vec![message_id.into(), run_id.into()],
    )
    .await?
      > 0,
  )
}

/// A summary its round let go without a line: it no longer awaits one and stands as the voice message it is.
pub async fn let_go(db: &DatabaseConnection, run_id: i64) -> Result<bool, StoreError> {
  Ok(
    run(
      db,
      r#"UPDATE "voice_briefings" SET "awaits_line" = 0 WHERE "run_id" = $1"#,
      vec![run_id.into()],
    )
    .await?
      > 0,
  )
}

/// The voice message a run went out as.
pub async fn sent(db: &DatabaseConnection, run_id: i64, voice_message_id: i64) -> Result<(), StoreError> {
  run(
    db,
    r#"UPDATE "voice_briefings" SET "voice_message_id" = $1 WHERE "run_id" = $2"#,
    vec![voice_message_id.into(), run_id.into()],
  )
  .await
  .map(drop)
}

/// When a run's voice went out and how long its sound runs, which the next summary of the room waits out.
pub async fn played(
  db: &DatabaseConnection,
  run_id: i64,
  sent_at: i64,
  seconds: Option<f64>,
) -> Result<(), StoreError> {
  run(
    db,
    r#"UPDATE "voice_briefings" SET "sent_at" = $1, "seconds" = $2 WHERE "run_id" = $3"#,
    vec![sent_at.into(), seconds.into(), run_id.into()],
  )
  .await
  .map(drop)
}

/// Where a full reading's or a narration's sound is kept.
pub async fn kept(db: &DatabaseConnection, run_id: i64, sound_path: &str) -> Result<(), StoreError> {
  run(
    db,
    r#"UPDATE "voice_briefings" SET "sound_path" = $1 WHERE "run_id" = $2"#,
    vec![sound_path.into(), run_id.into()],
  )
  .await
  .map(drop)
}

/// The run ended, however it ended.
pub async fn finished(db: &DatabaseConnection, run_id: i64, finished_at: i64) -> Result<(), StoreError> {
  run(
    db,
    r#"UPDATE "voice_briefings" SET "finished_at" = $1 WHERE "run_id" = $2 AND "finished_at" IS NULL"#,
    vec![finished_at.into(), run_id.into()],
  )
  .await
  .map(drop)
}

/// Whether a summary of the same room was launched after this one: the one that replaces it while it has not
/// gone out.
pub async fn newer_summary(db: &DatabaseConnection, chat_id: i64, run_id: i64) -> Result<bool, StoreError> {
  Ok(
    one(
      db,
      r#"SELECT "run_id" FROM "voice_briefings" WHERE "chat_id" = $1 AND "kind" = 'summary' AND "run_id" > $2 LIMIT 1"#,
      vec![chat_id.into(), run_id.into()],
    )
    .await?
    .is_some(),
  )
}

/// Whether any summary, of any room, is still being made: a full reading and a narration wait behind it.
pub async fn summary_being_made(db: &DatabaseConnection) -> Result<bool, StoreError> {
  Ok(
    one(
      db,
      r#"SELECT count(*) AS "count" FROM "voice_briefings"
         WHERE "kind" = 'summary' AND "finished_at" IS NULL AND "sent_at" IS NULL"#,
      vec![],
    )
    .await?
    .map(|row| row.try_get::<i64>("", "count"))
    .transpose()?
    .unwrap_or(0)
      > 0,
  )
}

/// The summaries of the same room launched before this one: how many are still being made, and the moment the
/// last of those that went out has been heard to its end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Earlier {
  pub making: i64,
  pub heard_until: Option<i64>,
}

pub async fn earlier_summaries(
  db: &DatabaseConnection,
  chat_id: i64,
  run_id: i64,
) -> Result<Earlier, StoreError> {
  let making = one(
    db,
    r#"SELECT count(*) AS "count" FROM "voice_briefings"
       WHERE "chat_id" = $1 AND "kind" = 'summary' AND "run_id" < $2 AND "finished_at" IS NULL AND "sent_at" IS NULL"#,
    vec![chat_id.into(), run_id.into()],
  )
  .await?
  .map(|row| row.try_get::<i64>("", "count"))
  .transpose()?
  .unwrap_or(0);
  let heard_until = one(
    db,
    r#"SELECT max("sent_at" + CAST(coalesce("seconds", 0) * 1000 AS bigint)) AS "until" FROM "voice_briefings"
       WHERE "chat_id" = $1 AND "kind" = 'summary' AND "run_id" < $2 AND "sent_at" IS NOT NULL"#,
    vec![chat_id.into(), run_id.into()],
  )
  .await?
  .map(|row| row.try_get::<Option<i64>>("", "until"))
  .transpose()?
  .flatten();

  Ok(Earlier {
    making,
    heard_until,
  })
}
