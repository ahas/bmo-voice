//! The studio's data root: the source manifest, the download records, and the review database the server shares with
//! the import script, which keeps the reviews and the sessions (docs/script-studio.md, 「Access and service」 and
//! 「Media and downloads」).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};

use super::review::{Fault, empty_review, validate_review};

/// The group of a file that stands at the top of the material.
const TOP_GROUP: &str = "안내";

/// One file of the provided material.
#[derive(Clone, Debug)]
pub struct Asset {
  pub id: String,
  pub path: String,
  pub title: String,
  pub group: String,
  pub size: Value,
  pub video: bool,
}

impl Asset {
  /// The asset as the screen reads it, its fields in their order.
  pub fn fields(&self) -> Map<String, Value> {
    let mut fields = Map::new();

    fields.insert("id".into(), json!(self.id));
    fields.insert("path".into(), json!(self.path));
    fields.insert("title".into(), json!(self.title));
    fields.insert("group".into(), json!(self.group));
    fields.insert("size".into(), self.size.clone());
    fields.insert("video".into(), json!(self.video));
    fields
  }
}

/// Why the records did not answer: a review that does not read, or the store or a file failing.
#[derive(Debug)]
pub enum RecordError {
  Review(Fault),
  Store(String),
}

impl From<Fault> for RecordError {
  fn from(fault: Fault) -> Self {
    Self::Review(fault)
  }
}

impl std::fmt::Display for RecordError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Review(fault) => fault.fmt(f),
      Self::Store(why) => f.write_str(why),
    }
  }
}

fn store(fault: impl std::fmt::Display) -> RecordError {
  RecordError::Store(fault.to_string())
}

fn read_json(path: &Path) -> Option<Value> {
  serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The data root and the folder the source manifest's paths stand under.
pub struct Root {
  pub path: PathBuf,
  source_root: String,
}

impl Root {
  /// A data root whose manifest names its files under `source_root`, empty where they stand at the top.
  pub fn new(path: &Path, source_root: Option<&str>) -> Self {
    let source_root = source_root.map(|root| root.trim_matches('/')).filter(|root| !root.is_empty());

    Self {
      path: path.to_owned(),
      source_root: source_root.map_or_else(String::new, |root| format!("{root}/")),
    }
  }

  /// The key an asset's path stands under in the manifest and the download records.
  pub fn key(&self, asset: &Asset) -> String {
    format!("{}{}", self.source_root, asset.path)
  }

  /// Every file of the source manifest, named by the hash of its path there.
  pub fn assets(&self) -> Result<Vec<Asset>, String> {
    let manifest = self.path.join("source-manifest.json");

    if !manifest.exists() {
      return Ok(Vec::new());
    }

    let rows: Value =
      serde_json::from_str(&std::fs::read_to_string(&manifest).map_err(|fault| fault.to_string())?)
        .map_err(|fault| fault.to_string())?;
    let rows = rows.as_array().ok_or("the source manifest is no list")?;

    rows
      .iter()
      .map(|row| {
        let source = row["path"].as_str().ok_or("a source path is not text")?;
        let path = source.strip_prefix(self.source_root.as_str()).unwrap_or(source).to_owned();

        if path.contains("..") || path.starts_with('/') {
          return Err(format!("{source} leaves the material"));
        }

        let parts: Vec<&str> = path.split('/').collect();
        let group = parts[..parts.len() - 1].join("/");

        Ok(Asset {
          id: format!("{:x}", Sha256::digest(source.as_bytes())),
          title: parts[parts.len() - 1].to_owned(),
          group: if group.is_empty() { TOP_GROUP.to_owned() } else { group },
          size: row.get("size").cloned().unwrap_or(Value::Null),
          video: path.to_ascii_lowercase().ends_with(".mp4"),
          path,
        })
      })
      .collect()
  }

  /// The download record of one asset, if any.
  pub fn download_of(&self, asset: &Asset) -> Option<Value> {
    read_json(&self.path.join("downloads.json"))?.get(self.key(asset)).cloned()
  }
}

pub struct Records {
  pub root: Root,
  db: SqlitePool,
}

impl Records {
  /// The review database, made where none stands. It has several writers, the server and the import script, so a
  /// writer waits for the lock rather than failing, and the write-ahead log is kept from growing without bound.
  pub async fn open(root: Root) -> Result<Records, sqlx::Error> {
    let options = SqliteConnectOptions::new()
      .filename(root.path.join("studio.db"))
      .create_if_missing(true)
      .journal_mode(SqliteJournalMode::Wal)
      .busy_timeout(std::time::Duration::from_secs(10))
      .pragma("journal_size_limit", (32 * 1024 * 1024).to_string());
    let db = SqlitePoolOptions::new().max_connections(4).connect_with(options).await?;

    sqlx::query("CREATE TABLE IF NOT EXISTS reviews(id TEXT PRIMARY KEY, version INTEGER NOT NULL, body TEXT NOT NULL, updated_at INTEGER NOT NULL)")
      .execute(&db)
      .await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS sessions(token TEXT PRIMARY KEY, name TEXT NOT NULL, expires INTEGER NOT NULL)")
      .execute(&db)
      .await?;

    Ok(Records { root, db })
  }

  /// One asset's review, or the empty one where none was written.
  pub async fn read(&self, id: &str) -> Result<Value, RecordError> {
    let row = sqlx::query("SELECT body FROM reviews WHERE id=?")
      .bind(id)
      .fetch_optional(&self.db)
      .await
      .map_err(store)?;

    match row {
      None => Ok(empty_review()),
      Some(row) => {
        let body: String = row.try_get("body").map_err(store)?;

        Ok(validate_review(&serde_json::from_str(&body).map_err(store)?)?)
      }
    }
  }

  /// Writes a review edited from the version standing, as the next version; one saved in between is refused, so one
  /// tab never overwrites another's save.
  pub async fn write(&self, id: &str, body: &Value) -> Result<Value, RecordError> {
    let review = validate_review(body)?;
    let mut connection = self.db.acquire().await.map_err(store)?;

    sqlx::query("BEGIN IMMEDIATE").execute(&mut *connection).await.map_err(store)?;

    let written = async {
      let row = sqlx::query("SELECT body FROM reviews WHERE id=?")
        .bind(id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(store)?;
      let standing = match row {
        None => empty_review(),
        Some(row) => {
          validate_review(&serde_json::from_str(&row.try_get::<String, _>("body").map_err(store)?).map_err(store)?)?
        }
      };

      if standing["version"] != review["version"] {
        return Err(RecordError::Review(Fault::EditedElsewhere));
      }

      let mut next = review.clone();
      let version = review["version"].as_i64().unwrap_or(0) + 1;

      next["version"] = json!(version);
      sqlx::query("INSERT INTO reviews VALUES(?,?,?,?) ON CONFLICT(id) DO UPDATE SET version=excluded.version,body=excluded.body,updated_at=excluded.updated_at")
        .bind(id)
        .bind(version)
        .bind(next.to_string())
        .bind(crate::now_ms())
        .execute(&mut *connection)
        .await
        .map_err(store)?;

      Ok(next)
    }
    .await;
    let ending = if written.is_ok() { "COMMIT" } else { "ROLLBACK" };

    sqlx::query(ending).execute(&mut *connection).await.map_err(store)?;
    written
  }

  /// Every asset with where its download stands and what its review holds, as the screen lists them.
  pub async fn list(&self) -> Result<Vec<Value>, RecordError> {
    let downloads = read_json(&self.root.path.join("downloads.json")).unwrap_or_else(|| json!({}));
    let sources = read_json(&self.root.path.join("research-index.json"))
      .and_then(|index| index.get("sources").cloned())
      .and_then(|sources| sources.as_array().cloned())
      .unwrap_or_default();
    let mut listed = Vec::new();

    for asset in self.root.assets().map_err(RecordError::Store)? {
      let key = self.root.key(&asset);
      let download = downloads.get(&key);
      let review = self.read(&asset.id).await?;
      let status = download.and_then(|one| one.get("status")).cloned().unwrap_or(json!("queued"));
      let mut received = download
        .and_then(|one| one.get("bytes"))
        .cloned()
        .filter(|bytes| !bytes.is_null())
        .unwrap_or(json!(0));

      if status != "complete"
        && let Ok(meta) =
          std::fs::metadata(self.root.path.join("media").join(format!("{}.part", asset.path)))
      {
        received = json!(meta.len());
      }

      let research: Vec<Value> = sources
        .iter()
        .filter(|source| source.get("path").and_then(Value::as_str) == Some(key.as_str()))
        .map(|source| {
          // A field the record lacks stays away, as it does when the screen copies the record.
          let kept: Map<String, Value> = ["title", "url"]
            .iter()
            .filter_map(|key| Some(((*key).to_owned(), source.get(*key)?.clone())))
            .collect();

          Value::Object(kept)
        })
        .collect();
      let mut entry = asset.fields();

      entry.insert("researchSources".into(), Value::Array(research));
      entry.insert("status".into(), status);
      entry.insert("received".into(), received);
      entry.insert(
        "error".into(),
        download.and_then(|one| one.get("error")).cloned().unwrap_or(Value::Null),
      );
      entry.insert(
        "tracks".into(),
        json!(download.and_then(|one| one.get("tracks")).and_then(Value::as_array).map_or(0, Vec::len)),
      );
      entry.insert("appearance".into(), review["appearance"].clone());
      entry.insert("speech".into(), review["speech"].clone());
      entry.insert("lineCount".into(), json!(review["lines"].as_array().map_or(0, Vec::len)));
      entry.insert("version".into(), review["version"].clone());
      listed.push(Value::Object(entry));
    }

    Ok(listed)
  }

  /// Who a session's token belongs to, or none for no live session.
  pub async fn session_of(&self, hashed_token: &str, now: i64) -> Option<String> {
    sqlx::query("SELECT name FROM sessions WHERE token=? AND expires>?")
      .bind(hashed_token)
      .bind(now)
      .fetch_optional(&self.db)
      .await
      .ok()??
      .try_get("name")
      .ok()
  }

  pub async fn open_session(
    &self,
    hashed_token: &str,
    name: &str,
    now: i64,
    expires: i64,
  ) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM sessions WHERE expires<?").bind(now).execute(&self.db).await?;
    sqlx::query("INSERT INTO sessions VALUES(?,?,?)")
      .bind(hashed_token)
      .bind(name)
      .bind(expires)
      .execute(&self.db)
      .await?;

    Ok(())
  }

  pub async fn close_session(&self, hashed_token: &str) {
    let _ = sqlx::query("DELETE FROM sessions WHERE token=?")
      .bind(hashed_token)
      .execute(&self.db)
      .await;
  }
}
