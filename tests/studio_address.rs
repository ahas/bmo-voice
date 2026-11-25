//! Where the script studio listens (`docs/script-studio.md`, 「Access and service」): the port its environment names,
//! else the settings' `[studio]` port, else 3214; `script-studio address` prints that address from the same settings,
//! which the deployment's health asks the studio at.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use bmo_voice::settings::{SETTINGS_VARIABLE, Settings, Studio};

static MADE: AtomicU32 = AtomicU32::new(0);

/// A scratch folder of this test's own, removed once dropped.
struct Scratch(PathBuf);

impl Scratch {
  fn new() -> Self {
    let made = MADE.fetch_add(1, Ordering::SeqCst);
    let dir =
      std::env::temp_dir().join(format!("bmo_voice_studio_address_test_{}_{made}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the scratch folder is made");
    Self(dir)
  }

  fn path(&self) -> &Path {
    &self.0
  }
}

impl Drop for Scratch {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

/// Settings in `dir` holding `studio`, the `[studio]` table's lines, and the keys every settings file needs.
fn settings_in(dir: &Path, studio: &str) -> PathBuf {
  let path = dir.join("voice.toml");
  let text = format!(
    r#"listen = "127.0.0.1:0"
address = "http://127.0.0.1:0"
token_file = "voice.token"
database = "postgres:///unused"
data_root = "{}"

[units.node]
address = "http://[::1]:9"

[units.arbiter]
address = "http://[::1]:9"

[units.web]
address = "http://[::1]:9"

[studio]
{studio}
"#,
    dir.display()
  );
  std::fs::write(&path, text).expect("the settings are written");
  path
}

fn address_by(settings: &Path, named_port: Option<&str>) -> String {
  let mut command = Command::new(env!("CARGO_BIN_EXE_script-studio"));
  command
    .arg("address")
    .env(SETTINGS_VARIABLE, settings)
    .env_remove("BMO_VOICE_STUDIO_PORT");
  if let Some(port) = named_port {
    command.env("BMO_VOICE_STUDIO_PORT", port);
  }
  let ran = command.output().expect("the studio's binary runs");
  assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
  String::from_utf8(ran.stdout).expect("the address is text")
}

#[test]
fn the_settings_port_stands_over_the_default_and_the_environment_over_both() {
  let dir = Scratch::new();
  let named = Settings::read(&settings_in(dir.path(), "port = 4321")).expect("the settings read");
  let unnamed = Settings::read(&settings_in(dir.path(), "")).expect("the settings read");

  assert_eq!(named.studio.listen_port(None), 4321);
  assert_eq!(named.studio.listen_port(Some(5000)), 5000);
  assert_eq!(unnamed.studio.listen_port(None), Studio::PORT);
  assert_eq!(Studio::PORT, 3214);
}

#[test]
fn the_studios_address_is_the_port_its_settings_name() {
  let dir = Scratch::new();

  let settings = settings_in(dir.path(), "port = 4321");
  assert_eq!(address_by(&settings, None), "http://127.0.0.1:4321\n");
  assert_eq!(address_by(&settings, Some("5000")), "http://127.0.0.1:5000\n");
  let settings = settings_in(dir.path(), "");
  assert_eq!(address_by(&settings, None), "http://127.0.0.1:3214\n");
}
