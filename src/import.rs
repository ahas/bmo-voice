//! The old archive's voice tables moved into this store (docs/store.md, and the migration plan's table map
//! for TODO #1923): one export directory read, this store written, nothing else touched.
//!
//! The export is the one the item's export hand wrote under
//! `workbench/worker/items/1923/exports/voice/`: `manifest.json` with each source table's exact count and
//! key, and one NDJSON file a table. The import reads only that directory, refuses a store already holding
//! rows of its tables, and runs whole inside one transaction, so a dry run and a failed run leave the store
//! as it stood. Every table keeps its old name; the archive's `rowid` is not carried, since the store
//! numbers the row.
//!
//! A briefing that carries no `awaits_line` is written at the column's default, `0`, as the store's own
//! migration gives it. The service's seeds of the name readings run as it starts and write nothing over a
//! name already standing, so the imported readings stand.

use std::collections::HashMap;
use std::path::Path;

use sea_orm::Database;
use serde_json::Value;
use sqlx::Row;
use sqlx::postgres::{PgPool, PgPoolOptions};

/// One table the import moves: its source name, its target name, and its columns.
pub struct TableMap {
  pub source: &'static str,
  pub target: &'static str,
  pub columns: &'static [&'static str],
}

/// Every table this import moves.
pub const TABLES: &[TableMap] = &[
  TableMap {
    source: "voices",
    target: "voices",
    columns: &[
      "name",
      "checkpoint",
      "reference",
      "cfg",
      "steps",
      "current",
      "set_at",
      "style",
      "seed",
      "by_sentence",
      "prompt_clip",
      "prompt_words",
      "prompt_en_clip",
      "prompt_en_words",
    ],
  },
  TableMap {
    source: "voice_names",
    target: "voice_names",
    columns: &["written", "ko", "en", "set_at"],
  },
  TableMap {
    source: "voice_nodes",
    target: "voice_nodes",
    columns: &["name", "position", "added_at"],
  },
  TableMap {
    source: "voice_briefings",
    target: "voice_briefings",
    columns: &[
      "run_id",
      "chat_id",
      "message_id",
      "kind",
      "title",
      "voice_message_id",
      "sent_at",
      "seconds",
      "sound_path",
      "awaits_line",
      "started_at",
      "finished_at",
    ],
  },
];

/// One export table's entry in the manifest.
#[derive(Clone, Debug)]
pub struct TableEntry {
  pub file: String,
  pub count: u64,
  pub key: Vec<String>,
}

/// The export directory's manifest.
#[derive(Clone, Debug)]
pub struct Manifest {
  pub snapshot: String,
  pub read_at: String,
  pub tables: HashMap<String, TableEntry>,
}

/// One value written to the store.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
  Null,
  Text(String),
}

/// One mapped row.
pub type Mapped = Vec<(&'static str, Cell)>;

/// One table of the report.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
  pub source: String,
  pub exported: u64,
  pub written: u64,
}

/// What an import run did.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
  pub dry_run: bool,
  pub snapshot: String,
  pub read_at: String,
  pub tables: Vec<Moved>,
  pub sample: Sample,
}

impl Report {
  /// The report as the command prints it.
  pub fn lines(&self) -> Vec<String> {
    let mut lines = vec![format!(
      "{} at snapshot {}:",
      if self.dry_run { "dry run" } else { "import" },
      self.snapshot
    )];
    for table in &self.tables {
      lines.push(format!("  {}: {} exported, {} written", table.source, table.exported, table.written));
    }
    lines.push(format!("{} rows sampled, {} failures", self.sample.checked, self.sample.failures.len()));
    lines.extend(self.sample.failures.iter().map(|failure| format!("  {failure}")));

    lines
  }
}

/// The count-and-sample compare's result.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
  pub checked: usize,
  pub failures: Vec<String>,
}

fn cell(value: Option<&Value>) -> Cell {
  match value {
    None | Some(Value::Null) => Cell::Null,
    Some(Value::String(said)) => Cell::Text(said.clone()),
    Some(Value::Bool(flag)) => Cell::Text(flag.to_string()),
    Some(Value::Number(number)) => Cell::Text(number.to_string()),
    Some(other) => Cell::Text(other.to_string()),
  }
}

/// One source row written in its target's columns.
pub fn row(table: &TableMap, old: &Value) -> Mapped {
  table
    .columns
    .iter()
    .map(|column| {
      let mut cell = cell(old.get(*column));
      if table.target == "voice_briefings" && *column == "awaits_line" && cell == Cell::Null {
        // The column's default as the store's own migration gives it.
        cell = Cell::Text("0".to_owned());
      }
      (*column, cell)
    })
    .collect()
}

/// The export directory's manifest.
pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
  let path = dir.join("manifest.json");
  let said = std::fs::read_to_string(&path)
    .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
  let document: Value =
    serde_json::from_str(&said).map_err(|error| format!("{} is not JSON: {error}", path.display()))?;
  let mut tables = HashMap::new();
  let list = document.get("tables").and_then(Value::as_array).ok_or("the manifest names no tables")?;
  for table in list {
    let source = table
      .get("source")
      .and_then(Value::as_str)
      .ok_or("a manifest table names no source")?
      .to_owned();
    let file = table
      .get("file")
      .and_then(Value::as_str)
      .ok_or("a manifest table names no file")?
      .to_owned();
    let count = table.get("count").and_then(Value::as_u64).ok_or("a manifest table names no count")?;
    let key: Vec<String> = table
      .get("key")
      .and_then(Value::as_array)
      .map(|keys| keys.iter().filter_map(Value::as_str).map(str::to_owned).collect())
      .unwrap_or_default();
    tables.insert(source, TableEntry { file, count, key });
  }

  Ok(Manifest {
    snapshot: document.get("snapshot").and_then(Value::as_str).unwrap_or_default().to_owned(),
    read_at: document.get("readAt").and_then(Value::as_str).unwrap_or_default().to_owned(),
    tables,
  })
}

/// One table's rows, each checked against the manifest's count as it is read.
pub fn rows(dir: &Path, manifest: &Manifest, source: &str) -> Result<Vec<Value>, String> {
  let entry = manifest
    .tables
    .get(source)
    .ok_or_else(|| format!("the manifest has no table {source}"))?;
  let path = dir.join(&entry.file);
  let said = std::fs::read_to_string(&path)
    .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
  let mut out = Vec::new();
  for (number, line) in said.lines().enumerate() {
    if line.trim().is_empty() {
      continue;
    }
    let row: Value = serde_json::from_str(line)
      .map_err(|error| format!("{} line {} is not JSON: {error}", path.display(), number + 1))?;
    out.push(row);
  }
  if out.len() as u64 != entry.count {
    return Err(format!(
      "{}: the manifest says {} rows and the file holds {}",
      path.display(),
      entry.count,
      out.len()
    ));
  }

  Ok(out)
}

/// The export read and mapped, before anything is written.
pub struct Prepared {
  pub manifest: Manifest,
  pub tables: Vec<(&'static str, &'static str, Vec<Mapped>)>,
}

/// Reads the export and maps every row to its target's shape.
pub fn prepare(export: &Path) -> Result<Prepared, String> {
  let manifest = read_manifest(export)?;
  let mut tables = Vec::new();
  for table in TABLES {
    let old = rows(export, &manifest, table.source)?;
    let mapped: Vec<_> = old.iter().map(|old| row(table, old)).collect();
    tables.push((table.source, table.target, mapped));
  }

  Ok(Prepared { manifest, tables })
}

/// The target's columns and their types. A type of the database's own — `citext` here — is cast by its own
/// name.
async fn types(pool: &PgPool, table: &str) -> Result<HashMap<String, String>, String> {
  let rows = sqlx::query(
    "SELECT column_name, data_type, udt_name FROM information_schema.columns \
     WHERE table_schema = 'public' AND table_name = $1",
  )
  .bind(table)
  .fetch_all(pool)
  .await
  .map_err(|error| format!("{table}'s columns could not be read: {error}"))?;
  let mut out = HashMap::new();
  for row in rows {
    let data_type: String = row.get("data_type");
    let udt_name: String = row.get("udt_name");
    let kind = if data_type == "USER-DEFINED" { udt_name } else { data_type };
    out.insert(row.get::<String, _>("column_name"), kind);
  }
  if out.is_empty() {
    return Err(format!("{table} holds no columns"));
  }

  Ok(out)
}

async fn insert(
  tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
  table: &str,
  kinds: &HashMap<String, String>,
  row: &[(&'static str, Cell)],
) -> Result<(), String> {
  let columns = row.iter().map(|(column, _)| format!("\"{column}\"")).collect::<Vec<_>>().join(", ");
  let values = row
    .iter()
    .enumerate()
    .map(|(position, (column, _))| {
      let kind = kinds.get(*column).map(String::as_str).unwrap_or("text");
      format!("CAST(${} AS {kind})", position + 1)
    })
    .collect::<Vec<_>>()
    .join(", ");
  let sql = format!("INSERT INTO \"{table}\" ({columns}) VALUES ({values})");
  let mut query = sqlx::query(&sql);
  for (_, cell) in row {
    query = match cell {
      Cell::Null => query.bind(None::<String>),
      Cell::Text(said) => query.bind(said.clone()),
    };
  }
  query
    .execute(&mut **tx)
    .await
    .map_err(|error| format!("{table} could not be written: {error}"))?;

  Ok(())
}

/// Refuses a store that already holds rows of any table this import writes.
pub async fn refused_if_holding(pool: &PgPool) -> Result<(), String> {
  let mut holding = Vec::new();
  for table in TABLES {
    let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM \"{}\"", table.target))
      .fetch_one(pool)
      .await
      .map_err(|error| format!("{} could not be counted: {error}", table.target))?;
    if count > 0 {
      holding.push(format!("{} holds {count}", table.target));
    }
  }
  if holding.is_empty() {
    Ok(())
  } else {
    Err(format!(
      "the store already holds rows of its tables, so nothing was written: {}",
      holding.join(", ")
    ))
  }
}

// A stable digest of a row's key, so the same rows are sampled however the file is ordered; FNV-1a.
fn digest(said: &str) -> u64 {
  let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
  for byte in said.as_bytes() {
    hash ^= u64::from(*byte);
    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
  }

  hash
}

fn key_text(row: &[(&'static str, Cell)], key: &[String]) -> String {
  key
    .iter()
    .map(|column| {
      row
        .iter()
        .find(|(held, _)| held == column)
        .map(|(_, cell)| format!("{cell:?}"))
        .unwrap_or_default()
    })
    .collect::<Vec<_>>()
    .join("|")
}

fn sample<'a>(rows: &'a [Mapped], key: &[String]) -> Vec<&'a Mapped> {
  if rows.len() <= 20 {
    return rows.iter().collect();
  }
  let mut ordered: Vec<(u64, &Mapped)> =
    rows.iter().map(|row| (digest(&key_text(row, key)), row)).collect();
  ordered.sort_by_key(|(hash, _)| *hash);
  let mut picked: Vec<&Mapped> = vec![&rows[0], &rows[rows.len() - 1]];
  for (_, row) in ordered {
    if !picked.iter().any(|held| std::ptr::eq(*held, row)) {
      picked.push(row);
    }
    if picked.len() >= 22 {
      break;
    }
  }

  picked
}

async fn compare(pool: &PgPool, prepared: &Prepared) -> Sample {
  let mut failures = Vec::new();
  let mut checked = 0;
  for (source, target, rows) in &prepared.tables {
    let entry = match prepared.manifest.tables.get(*source) {
      Some(entry) => entry,
      None => continue,
    };
    match sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM \"{target}\""))
      .fetch_one(pool)
      .await
    {
      Ok(held) if held as u64 == entry.count => {}
      Ok(held) => {
        failures.push(format!("{source}: the export holds {} rows and {target} holds {held}", entry.count))
      }
      Err(error) => failures.push(format!("{source}: {target} could not be counted: {error}")),
    }
    if entry.key.is_empty() {
      failures.push(format!("{source}: the export records no key to sample by"));
      continue;
    }
    let kinds = match types(pool, target).await {
      Ok(kinds) => kinds,
      Err(error) => {
        failures.push(error);
        continue;
      }
    };
    for row in sample(rows, &entry.key) {
      checked += 1;
      let selected = row
        .iter()
        .map(|(column, _)| format!("\"{column}\"::text AS \"{column}\""))
        .collect::<Vec<_>>()
        .join(", ");
      let mut sql = format!("SELECT {selected} FROM \"{target}\" WHERE ");
      for (position, column) in entry.key.iter().enumerate() {
        if position > 0 {
          sql.push_str(" AND ");
        }
        let kind = kinds.get(column).map(String::as_str).unwrap_or("text");
        sql.push_str(&format!("\"{column}\" = CAST(${} AS {kind})", position + 1));
      }
      let mut query = sqlx::query(&sql);
      let mut bound = true;
      for column in &entry.key {
        match row.iter().find(|(held, _)| held == column).map(|(_, cell)| cell) {
          Some(Cell::Null) => {
            failures.push(format!("{source}: the sampled row carries no {column}"));
            bound = false;
            break;
          }
          Some(Cell::Text(said)) => query = query.bind(said.clone()),
          None => {
            failures.push(format!("{source}: the mapped row carries no {column}"));
            bound = false;
            break;
          }
        }
      }
      if !bound {
        continue;
      }
      match query.fetch_optional(pool).await {
        Ok(Some(held)) => {
          for (column, cell) in row {
            let actual = held.try_get::<Option<String>, _>(*column).ok().flatten();
            let expected = match cell {
              Cell::Null => None,
              Cell::Text(said) => Some(said.clone()),
            };
            if actual != expected {
              failures
                .push(format!("{source} -> {target}: {column} was written {expected:?} but is {actual:?}"));
            }
          }
        }
        Ok(None) => failures.push(format!("{source} -> {target}: a sampled row is missing")),
        Err(error) => failures.push(format!("{source} -> {target}: the sample could not be read: {error}")),
      }
    }
  }

  Sample { checked, failures }
}

/// Runs the import: reads the export, refuses a store holding rows, writes every row, and compares what it
/// wrote. A dry run rolls the transaction back and reports what would have been written.
pub async fn run(database: &str, export: &Path, dry_run: bool) -> Result<Report, String> {
  let prepared = prepare(export)?;
  // The store's own migrations, run over the database before anything is read into it.
  let migrated = Database::connect(database)
    .await
    .map_err(|error| format!("{database} could not be opened: {error}"))?;
  crate::store::migrate(&migrated)
    .await
    .map_err(|error| format!("the migrations: {error}"))?;
  migrated
    .close()
    .await
    .map_err(|error| format!("the migration connection did not close: {error}"))?;

  let pool = PgPoolOptions::new()
    .max_connections(1)
    .connect(database)
    .await
    .map_err(|error| format!("{database} could not be opened: {error}"))?;
  refused_if_holding(&pool).await?;

  let mut kinds: HashMap<&str, HashMap<String, String>> = HashMap::new();
  for (_, target, _) in &prepared.tables {
    kinds.insert(target, types(&pool, target).await?);
  }

  let mut tx = pool
    .begin()
    .await
    .map_err(|error| format!("the transaction could not be opened: {error}"))?;
  for (_, target, rows) in &prepared.tables {
    let table_kinds = kinds.get(target).expect("a target's types");
    for row in rows {
      insert(&mut tx, target, table_kinds, row).await?;
    }
  }
  if dry_run {
    tx.rollback()
      .await
      .map_err(|error| format!("the dry run could not be rolled back: {error}"))?;
  } else {
    tx.commit()
      .await
      .map_err(|error| format!("the import could not be committed: {error}"))?;
  }

  let sample = if dry_run {
    Sample {
      checked: 0,
      failures: Vec::new(),
    }
  } else {
    compare(&pool, &prepared).await
  };

  let tables: Vec<Moved> = prepared
    .tables
    .iter()
    .map(|(source, _target, rows)| Moved {
      source: (*source).to_owned(),
      exported: prepared.manifest.tables.get(*source).map(|entry| entry.count).unwrap_or(0),
      written: rows.len() as u64,
    })
    .collect();
  pool.close().await;

  Ok(Report {
    dry_run,
    snapshot: prepared.manifest.snapshot.clone(),
    read_at: prepared.manifest.read_at.clone(),
    tables,
    sample,
  })
}
