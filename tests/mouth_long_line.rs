//! A long line through the voice server's own `POST /speak` comes back as one sound (`tests/mouth_long_line.py`),
//! run on the Python tests' own interpreter: the one `BMO_VOICE_PYTHON` names, else `tests/.venv`, which the
//! declaration's setup makes from `tests/python-requirements.txt` (README.md, 「Tests」).

use std::path::PathBuf;
use std::process::Command;

pub(crate) fn voice_python() -> PathBuf {
  match std::env::var_os("BMO_VOICE_PYTHON") {
    Some(named) => PathBuf::from(named),
    None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/.venv/bin/python"),
  }
}

#[test]
fn a_line_of_any_length_the_engine_takes_in_comes_back_as_one_sound() {
  let python = voice_python();
  let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mouth_long_line.py");
  let ran = Command::new(&python)
    .arg(&script)
    .output()
    .unwrap_or_else(|why| panic!("the Python tests' interpreter {} does not run: {why}", python.display()));

  assert!(
    ran.status.success(),
    "the long line was not spoken whole:\n{}{}",
    String::from_utf8_lossy(&ran.stdout),
    String::from_utf8_lossy(&ran.stderr)
  );
}
