//! A database and a data root of a test's own (`docs/layout.md`, 「Tests」): made empty on the suite's server, named
//! after the harness's process, and dropped when the test ends; and, before the process makes its first, the
//! leftovers of the harness's processes that died before their teardown dropped.

use std::sync::Once;
use std::sync::atomic::{AtomicU32, Ordering};

use sqlx::postgres::PgConnection;
use sqlx::{Connection, Executor, Row};

/// What every scratch database's name starts with, followed by the harness's process id and a count.
const PREFIX: &str = "bmo_voice_test_";
/// How many times a drop clears the database's sessions and tries again before it gives up.
const DROP_ATTEMPTS: usize = 3;

static MADE: AtomicU32 = AtomicU32::new(0);
static SWEPT: Once = Once::new();

/// The suite's server, `BMO_TEST_POSTGRES` or the local socket's `bmo_test`.
fn server() -> String {
  std::env::var("BMO_TEST_POSTGRES")
    .ok()
    .filter(|named| !named.is_empty())
    .unwrap_or_else(|| "postgres:///bmo_test?host=/var/run/postgresql".into())
}

/// The address of `database` on the suite's server.
pub(crate) fn address_of(database: &str) -> String {
  let mut address = url::Url::parse(&server()).expect("the server's address reads");
  address.set_path(&format!("/{database}"));

  address.to_string()
}

/// A database and a data root of this test's own, made empty and dropped when the test ends.
pub(crate) struct Scratch {
  pub(crate) name: String,
  pub(crate) root: std::path::PathBuf,
}

impl Scratch {
  pub(crate) async fn new() -> Scratch {
    SWEPT.call_once(|| on_own_runtime(sweep()));

    let name = format!("{PREFIX}{}_{}", std::process::id(), MADE.fetch_add(1, Ordering::SeqCst));
    let mut admin = PgConnection::connect(&address_of("postgres")).await.expect("the server answers");
    admin
      .execute(format!("CREATE DATABASE \"{name}\" TEMPLATE template0").as_str())
      .await
      .expect("the database is made");
    admin.close().await.expect("the server connection closes");

    let root = std::env::temp_dir().join(&name);

    std::fs::create_dir_all(root.join("env")).expect("the data root is made");

    Scratch { name, root }
  }

  /// The address of this test's database.
  pub(crate) fn address(&self) -> String {
    address_of(&self.name)
  }
}

impl Drop for Scratch {
  fn drop(&mut self) {
    let name = std::mem::take(&mut self.name);
    let _ = std::fs::remove_dir_all(&self.root);
    let dropped = on_own_runtime(async move {
      let mut admin = PgConnection::connect(&address_of("postgres"))
        .await
        .map_err(|error| error.to_string())?;
      let dropped = drop_database(&mut admin, &name).await.map_err(|error| format!("{name}: {error}"));
      let _ = admin.close().await;

      dropped
    });

    if let Err(error) = dropped
      && !std::thread::panicking()
    {
      panic!("the scratch database is dropped: {error}");
    }
  }
}

/// Runs `work` to its end on a runtime of its own on a thread of its own, since a guard dropped inside a test's runtime
/// may not block on it.
fn on_own_runtime<T: Send + 'static>(work: impl Future<Output = T> + Send + 'static) -> T {
  std::thread::spawn(move || {
    tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .expect("a runtime")
      .block_on(work)
  })
  .join()
  .expect("the scratch work ends")
}

/// Drops `name`: this role's own sessions on it are ended first, since a drop that forces every session off needs the
/// right to end other roles' processes, an autovacuum worker's among them, which the suite's role need not hold; the
/// plain drop then stops an autovacuum worker itself and waits for the ended sessions to leave. A session that opened
/// between the two is ended on the next attempt.
async fn drop_database(admin: &mut PgConnection, name: &str) -> Result<(), sqlx::Error> {
  let mut attempt = 1;

  loop {
    sqlx::query(
      "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
       WHERE datname = $1 AND pid <> pg_backend_pid() AND usename = current_user",
    )
    .bind(name)
    .execute(&mut *admin)
    .await?;

    match admin.execute(format!("DROP DATABASE IF EXISTS \"{name}\"").as_str()).await {
      Ok(_) => return Ok(()),
      Err(error) if attempt == DROP_ATTEMPTS => return Err(error),
      Err(_) => attempt += 1,
    }
  }
}

/// Drops every scratch database a dead process of this harness left, and its data root. A database is left alone while
/// its process lives or any session is on it, so a run of another checkout's harness against the same server keeps its
/// own; this process has made none yet, so its own id counts as dead.
async fn sweep() {
  let Ok(mut admin) = PgConnection::connect(&address_of("postgres")).await else {
    return;
  };
  let left = sqlx::query(
    "SELECT datname FROM pg_database d WHERE starts_with(datname, $1) \
     AND NOT EXISTS (SELECT 1 FROM pg_stat_activity a WHERE a.datname = d.datname)",
  )
  .bind(PREFIX)
  .fetch_all(&mut admin)
  .await
  .unwrap_or_default();

  for row in left {
    let name: String = row.get("datname");

    if process_of(&name).is_some_and(lives) {
      continue;
    }

    if drop_database(&mut admin, &name).await.is_ok() {
      let _ = std::fs::remove_dir_all(std::env::temp_dir().join(&name));
    }
  }

  let _ = admin.close().await;
}

/// The process id a scratch database's name carries, if the name is one this harness makes.
fn process_of(name: &str) -> Option<u32> {
  let (process, count) = name.strip_prefix(PREFIX)?.split_once('_')?;
  count.parse::<u32>().ok()?;

  process.parse().ok()
}

/// Whether process `id`, other than this one, is running on this machine. Where the process table cannot be read every
/// process counts as alive, so nothing is dropped on a guess.
fn lives(id: u32) -> bool {
  let table = std::path::Path::new("/proc");

  id != std::process::id() && (!table.join("self").exists() || table.join(id.to_string()).exists())
}
