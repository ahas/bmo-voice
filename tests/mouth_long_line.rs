//! A long line through the voice server's own `POST /speak` comes back as one sound (`tests/mouth_long_line.py`),
//! run on the voice environment's interpreter: the one `BMO_VOICE_PYTHON` names, else `bmo-voice/.venv` under the
//! account's home (docs/voice-plan.md, 「The environments」).

use std::path::PathBuf;
use std::process::Command;

fn voice_python() -> PathBuf {
  if let Some(named) = std::env::var_os("BMO_VOICE_PYTHON") {
    return PathBuf::from(named);
  }

  let home = std::env::var_os("HOME").expect("a home to find the voice environment under");

  PathBuf::from(home).join("bmo-voice/.venv/bin/python")
}

#[test]
fn a_line_of_any_length_the_engine_takes_in_comes_back_as_one_sound() {
  let python = voice_python();
  let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mouth_long_line.py");
  let ran = Command::new(&python).arg(&script).output().unwrap_or_else(|why| {
    panic!("the voice environment's interpreter {} does not run: {why}", python.display())
  });

  assert!(
    ran.status.success(),
    "the long line was not spoken whole:\n{}{}",
    String::from_utf8_lossy(&ran.stdout),
    String::from_utf8_lossy(&ran.stderr)
  );
}
