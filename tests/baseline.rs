//! The schema's starting point (docs/store.md, 「The migrations」): a blank database gets the baseline whole, an install holding the
//! record of the last release before 1.0.0 is taken as it stands, and one holding part of it is refused.

use bmo_voice::store::migration::{BASELINE, BEFORE, MIGRATIONS_TABLE};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

use crate::scratch::Scratch;

const SCHEMA: &str = include_str!("../src/store/migration/baseline.sql");

// Every column and index of the store but the record's, in one line.
fn shape_sql() -> String {
  format!(
    "SELECT COALESCE((SELECT string_agg(table_name || '.' || column_name || ' ' || data_type || ' ' || \
     is_nullable || ' ' || COALESCE(column_default, ''), ', ' ORDER BY table_name, ordinal_position) \
     FROM information_schema.columns WHERE table_schema = current_schema() AND table_name <> '{MIGRATIONS_TABLE}'), '') \
     || ' / ' || COALESCE((SELECT string_agg(indexdef, ', ' ORDER BY indexname) FROM pg_indexes \
     WHERE schemaname = current_schema() AND tablename <> '{MIGRATIONS_TABLE}'), '') AS shape"
  )
}

async fn texts(db: &DatabaseConnection, sql: &str) -> Vec<String> {
  db.query_all(Statement::from_string(DbBackend::Postgres, sql.to_owned()))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get_by_index::<String>(0).unwrap())
    .collect()
}

async fn shape(db: &DatabaseConnection) -> Vec<String> {
  texts(db, &shape_sql()).await
}

async fn recorded(db: &DatabaseConnection) -> Vec<String> {
  texts(db, &format!(r#"SELECT "version" FROM "{MIGRATIONS_TABLE}" ORDER BY "version""#)).await
}

// A blank database holding the baseline's schema and a row, recorded as an install from before 1.0.0 that ran
// the first `through` migrations of its last release.
async fn installed(scratch: &Scratch, through: usize) -> DatabaseConnection {
  let db = bmo_voice::store::connect(&scratch.address(), 2).await.unwrap();
  db.execute_unprepared(SCHEMA).await.unwrap();
  db.execute_unprepared(&format!(
    r#"CREATE TABLE "{MIGRATIONS_TABLE}" ("version" varchar PRIMARY KEY, "applied_at" bigint NOT NULL)"#
  ))
  .await
  .unwrap();
  for version in &BEFORE[..through] {
    db.execute_unprepared(&format!(
      r#"INSERT INTO "{MIGRATIONS_TABLE}" ("version", "applied_at") VALUES ('{version}', 1)"#
    ))
    .await
    .unwrap();
  }
  db.execute_unprepared("INSERT INTO voice_events (kind, at, data) VALUES ('a-kind', 1, '{}')")
    .await
    .unwrap();
  db
}

#[tokio::test]
async fn a_blank_database_gets_the_baseline_whole() {
  let migrated_scratch = Scratch::new().await;
  let scratch = &migrated_scratch;
  let migrated = bmo_voice::store::connect(&scratch.address(), 2).await.unwrap();
  bmo_voice::store::migration::up(&migrated).await.unwrap();
  let laid_scratch = Scratch::new().await;
  let scratch = &laid_scratch;
  let laid = bmo_voice::store::connect(&scratch.address(), 2).await.unwrap();
  laid.execute_unprepared(SCHEMA).await.unwrap();

  assert_eq!(shape(&migrated).await, shape(&laid).await);
  assert_eq!(recorded(&migrated).await, [BASELINE]);
}

#[tokio::test]
async fn an_install_at_the_last_release_before_the_baseline_only_has_its_record_rewritten() {
  let scratch = Scratch::new().await;
  let db = installed(&scratch, BEFORE.len()).await;
  let before = shape(&db).await;

  bmo_voice::store::migration::up(&db).await.unwrap();
  assert_eq!(recorded(&db).await, [BASELINE]);
  assert_eq!(shape(&db).await, before);
  assert_eq!(texts(&db, "SELECT COUNT(*)::text FROM voice_events").await, ["1"]);

  // A second start finds the baseline recorded and changes nothing.
  bmo_voice::store::migration::up(&db).await.unwrap();
  assert_eq!(recorded(&db).await, [BASELINE]);
  assert_eq!(shape(&db).await, before);
}

#[tokio::test]
async fn an_install_short_of_the_last_release_before_the_baseline_is_refused() {
  let scratch = Scratch::new().await;
  let db = installed(&scratch, 3).await;

  let refused = bmo_voice::store::migration::up(&db).await.unwrap_err().to_string();
  assert!(refused.contains(BEFORE[2]), "{refused}");
  assert_eq!(recorded(&db).await.len(), 3);
}
