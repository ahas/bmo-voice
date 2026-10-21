//! The old archive's four voice tables moved into the store (TODO #1923): the mapping, the briefing's
//! `awaits_line` at its default, the refusal of a store already holding rows, the dry run, and the
//! count-and-sample compare.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor};

static MADE: AtomicU32 = AtomicU32::new(0);

/// The test server's address, as the suite names it.
fn server() -> String {
  std::env::var("BMO_TEST_POSTGRES")
    .ok()
    .filter(|named| !named.is_empty())
    .unwrap_or_else(|| "postgres:///bmo_test?host=/var/run/postgresql".into())
}

fn address_of(database: &str) -> String {
  let mut address = url::Url::parse(&server()).expect("the server's address reads");
  address.set_path(&format!("/{database}"));

  address.to_string()
}

/// A database of this test's own, made empty and dropped when the test ends.
struct Scratch {
  name: String,
}

impl Scratch {
  async fn new() -> Scratch {
    let name = format!(
      "bmo_item_1923_voice_test_{}_{}",
      std::process::id(),
      MADE.fetch_add(1, Ordering::SeqCst)
    );
    let mut admin = PgConnection::connect(&address_of("postgres")).await.expect("the server answers");
    let _ = admin.execute(format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)").as_str()).await;
    admin
      .execute(format!("CREATE DATABASE \"{name}\" TEMPLATE template0").as_str())
      .await
      .expect("the database is made");
    admin.close().await.expect("the server connection closes");

    Scratch { name }
  }

  fn address(&self) -> String {
    address_of(&self.name)
  }
}

impl Drop for Scratch {
  fn drop(&mut self) {
    let name = std::mem::take(&mut self.name);
    // A copy is dropped on a thread of its own: a guard dropped inside the runtime may not block on it.
    let _ = std::thread::spawn(move || {
      let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a runtime");
      runtime.block_on(async move {
        if let Ok(mut admin) = PgConnection::connect(&address_of("postgres")).await {
          let _ = admin.execute(format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)").as_str()).await;
          let _ = admin.close().await;
        }
      });
    })
    .join();
  }
}

const VOICE: &str = r#"{"name":"bmo-005-e-1000","checkpoint":"/data/voices/bmo-005-e-1000","reference":"/data/voices/reference/one.wav","cfg":1.5,"steps":16,"current":1,"set_at":1791866256,"style":"calm","seed":0,"by_sentence":1,"prompt_clip":"","prompt_words":"","prompt_en_clip":"","prompt_en_words":"","rowid":1}"#;
const NAME: &str = r#"{"written":"BMO","ko":"비모","en":"Beemo","set_at":1791866256,"rowid":1}"#;
const NODE: &str = r#"{"name":"n2","position":6,"added_at":1791866256,"rowid":1}"#;
const BRIEFING: &str = r#"{"run_id":1,"chat_id":90000001,"message_id":2,"kind":"full","title":"a briefing","voice_message_id":3,"sent_at":1791866256,"seconds":12.5,"sound_path":"collected/90000001/2.ogg","awaits_line":0,"rowid":1}"#;

fn export(folder: &Path) {
  let tables: [(&str, &str, &str); 4] = [
    ("voices", "[\"name\"]", VOICE),
    ("voice_names", "[\"written\"]", NAME),
    ("voice_nodes", "[\"name\"]", NODE),
    ("voice_briefings", "[\"run_id\"]", BRIEFING),
  ];
  let mut manifest = Vec::new();
  for (source, key, row) in tables {
    manifest.push(format!(r#"{{"source":"{source}","file":"{source}.ndjson","count":1,"key":{key}}}"#));
    std::fs::write(folder.join(format!("{source}.ndjson")), format!("{row}\n"))
      .expect("the row is written");
  }
  let said = format!(
    r#"{{"snapshot":"23569147:23569147:","readAt":"2026-10-21T16:59:52+09:00","tables":[{}]}}"#,
    manifest.join(",")
  );
  std::fs::write(folder.join("manifest.json"), said).expect("the manifest is written");
}

fn folder(name: &str) -> std::path::PathBuf {
  let folder = std::env::temp_dir().join(format!("bmo-voice-import-{}-{name}", std::process::id()));
  let _ = std::fs::remove_dir_all(&folder);
  std::fs::create_dir_all(&folder).expect("the scratch folder is made");

  folder
}

#[tokio::test]
async fn the_four_tables_are_moved_and_compared() {
  let folder = folder("samples");
  export(&folder);
  let scratch = Scratch::new().await;

  let report = bmo_voice::import::run(&scratch.address(), &folder, false)
    .await
    .expect("the import runs");
  assert_eq!(report.tables.iter().map(|table| table.written).collect::<Vec<_>>(), [1, 1, 1, 1]);
  assert_eq!(report.sample.checked, 4);
  assert_eq!(report.sample.failures, Vec::<String>::new());

  let mut connection = PgConnection::connect(&scratch.address()).await.expect("the store opens");
  let name: String = sqlx::query_scalar("SELECT ko FROM voice_names WHERE written = 'BMO'")
    .fetch_one(&mut connection)
    .await
    .expect("the reading is read");
  assert_eq!(name, "비모");
  let awaits: i64 = sqlx::query_scalar("SELECT awaits_line FROM voice_briefings WHERE run_id = 1")
    .fetch_one(&mut connection)
    .await
    .expect("the briefing is read");
  assert_eq!(awaits, 0);
  let _ = connection.close().await;
}

#[tokio::test]
async fn a_briefing_without_awaits_line_is_written_at_its_default() {
  let folder = folder("default");
  export(&folder);
  // The same export with the column absent from the row.
  let said = std::fs::read_to_string(folder.join("voice_briefings.ndjson")).expect("the row reads");
  std::fs::write(folder.join("voice_briefings.ndjson"), said.replace(r#","awaits_line":0"#, ""))
    .expect("the row is rewritten");
  let scratch = Scratch::new().await;

  bmo_voice::import::run(&scratch.address(), &folder, false)
    .await
    .expect("the import runs");

  let mut connection = PgConnection::connect(&scratch.address()).await.expect("the store opens");
  let awaits: i64 = sqlx::query_scalar("SELECT awaits_line FROM voice_briefings WHERE run_id = 1")
    .fetch_one(&mut connection)
    .await
    .expect("the briefing is read");
  assert_eq!(awaits, 0);
  let _ = connection.close().await;
}

#[tokio::test]
async fn a_dry_run_writes_nothing() {
  let folder = folder("dry");
  export(&folder);
  let scratch = Scratch::new().await;

  bmo_voice::import::run(&scratch.address(), &folder, true)
    .await
    .expect("the dry run runs");

  let mut connection = PgConnection::connect(&scratch.address()).await.expect("the store opens");
  let held: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voices")
    .fetch_one(&mut connection)
    .await
    .expect("the count is read");
  assert_eq!(held, 0);
  let _ = connection.close().await;
}

#[tokio::test]
async fn a_store_holding_rows_is_refused() {
  let folder = folder("refuse");
  export(&folder);
  let scratch = Scratch::new().await;

  bmo_voice::import::run(&scratch.address(), &folder, false)
    .await
    .expect("the first import runs");
  let refused = bmo_voice::import::run(&scratch.address(), &folder, false)
    .await
    .expect_err("the second is refused");
  assert!(refused.contains("already holds rows"), "{refused}");
  assert!(refused.contains("voices holds 1"), "{refused}");
}
