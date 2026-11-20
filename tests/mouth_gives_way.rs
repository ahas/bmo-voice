//! The voice server's model as it gives the card way (`tests/mouth_gives_way.py`), run on the Python tests' own
//! interpreter as the long line is (docs/voice-plan.md, 「How it holds the card」).

use std::path::PathBuf;
use std::process::Command;

use super::mouth_long_line::voice_python;

#[test]
fn the_model_is_let_go_whole_as_the_card_gives_way_and_never_under_a_line() {
  let python = voice_python();
  let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mouth_gives_way.py");
  let ran = Command::new(&python)
    .arg(&script)
    .output()
    .unwrap_or_else(|why| panic!("the Python tests' interpreter {} does not run: {why}", python.display()));

  assert!(
    ran.status.success(),
    "the model did not give way as it should:\n{}{}",
    String::from_utf8_lossy(&ran.stdout),
    String::from_utf8_lossy(&ran.stderr)
  );
}
