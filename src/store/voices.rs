//! The voices table's rows, its names and the voice node list (`docs/store.md`; `docs/voice-plan.md`, 「The
//! voices」, 「How names are said」, 「The voice nodes」).

use sea_orm::{DatabaseConnection, QueryResult};
use serde::Deserialize;

use super::{StoreError, all, one, run};
pub use crate::script::reading::VoiceName;

/// The seed names a table first made is given (`data/ko/voice-names.toml`).
const SEEDS: &str = include_str!("../../data/ko/voice-names.toml");

#[derive(Deserialize)]
struct Seeds {
  name: Vec<VoiceName>,
}

/// The seed names, as the data file holds them.
pub fn seeds() -> Vec<VoiceName> {
  toml::from_str::<Seeds>(SEEDS)
    .unwrap_or_else(|why| panic!("data/ko/voice-names.toml does not read: {why}"))
    .name
}

/// One continuation prompt of a voice: the clip it starts from and the words that clip says.
#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
  pub clip: String,
  pub words: String,
}

/// One trained voice as its row names it: where its weights and its reference clip are, how it is asked to speak,
/// and its continuation prompts by language, Korean first.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceRow {
  pub name: String,
  pub checkpoint: String,
  pub reference: String,
  pub cfg: f64,
  pub steps: i64,
  pub style: String,
  pub seed: i64,
  pub prompts: Vec<(&'static str, Prompt)>,
  pub by_sentence: bool,
}

// The prompt columns of each language, in the order a voice is handed them.
const PROMPT_COLUMNS: [(&str, &str, &str); 2] =
  [("ko", "prompt_clip", "prompt_words"), ("en", "prompt_en_clip", "prompt_en_words")];

fn voice_row(row: &QueryResult) -> Result<VoiceRow, StoreError> {
  let mut prompts = vec![];

  for (language, clip, words) in PROMPT_COLUMNS {
    let clip: String = row.try_get("", clip)?;
    let words: String = row.try_get("", words)?;

    if !clip.is_empty() || !words.is_empty() {
      prompts.push((language, Prompt { clip, words }));
    }
  }

  Ok(VoiceRow {
    name: row.try_get("", "name")?,
    checkpoint: row.try_get("", "checkpoint")?,
    reference: row.try_get("", "reference")?,
    cfg: row.try_get("", "cfg")?,
    steps: row.try_get("", "steps")?,
    style: row.try_get("", "style")?,
    seed: row.try_get("", "seed")?,
    prompts,
    by_sentence: row.try_get::<i64>("", "by_sentence")? == 1,
  })
}

/// The voice of a name, or none where the table holds no such voice.
pub async fn voice(db: &DatabaseConnection, name: &str) -> Result<Option<VoiceRow>, StoreError> {
  one(db, r#"SELECT * FROM "voices" WHERE "name" = $1"#, vec![name.into()])
    .await?
    .as_ref()
    .map(voice_row)
    .transpose()
}

/// The names of every voice the table holds, in order.
pub async fn voice_names(db: &DatabaseConnection) -> Result<Vec<String>, StoreError> {
  all(db, r#"SELECT "name" FROM "voices" ORDER BY "name""#, vec![])
    .await?
    .iter()
    .map(|row| row.try_get("", "name").map_err(StoreError::from))
    .collect()
}

/// The voice marked current, which speaks for the chat agent where its registration names none.
pub async fn current_voice(db: &DatabaseConnection) -> Result<Option<String>, StoreError> {
  one(db, r#"SELECT "name" FROM "voices" WHERE "current" = 1 LIMIT 1"#, vec![])
    .await?
    .map(|row| row.try_get::<String>("", "name"))
    .transpose()
    .map_err(StoreError::from)
}

/// How the names the voice says are said in each language.
pub async fn said_names(db: &DatabaseConnection) -> Result<Vec<VoiceName>, StoreError> {
  all(
    db,
    r#"SELECT "written"::text AS "written", "ko", "en" FROM "voice_names" ORDER BY "written""#,
    vec![],
  )
  .await?
  .iter()
  .map(|row| {
    Ok(VoiceName {
      written: row.try_get("", "written")?,
      ko: row.try_get("", "ko")?,
      en: row.try_get("", "en")?,
    })
  })
  .collect()
}

/// A names table first made given its seed names; a table that holds any row is left as it is. Answers whether
/// it seeded.
pub async fn seed_names(
  db: &DatabaseConnection,
  seeds: &[VoiceName],
  now: i64,
) -> Result<bool, StoreError> {
  if one(db, r#"SELECT 1 AS "held" FROM "voice_names" LIMIT 1"#, vec![]).await?.is_some() {
    return Ok(false);
  }

  for seed in seeds {
    run(
      db,
      r#"INSERT INTO "voice_names" ("written", "ko", "en", "set_at") VALUES ($1, $2, $3, $4)
         ON CONFLICT ("written") DO NOTHING"#,
      vec![seed.written.clone().into(), seed.ko.clone().into(), seed.en.clone().into(), now.into()],
    )
    .await?;
  }

  Ok(true)
}

/// The voice node list, in the order a free node is taken; empty where the voice is spoken on this machine.
pub async fn node_names(db: &DatabaseConnection) -> Result<Vec<String>, StoreError> {
  all(db, r#"SELECT "name" FROM "voice_nodes" ORDER BY "position", "name""#, vec![])
    .await?
    .iter()
    .map(|row| row.try_get("", "name").map_err(StoreError::from))
    .collect()
}
