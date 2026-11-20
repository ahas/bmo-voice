//! The voice server's account of a line (`tests/mouth_stages.py`), run on the Python tests' own interpreter as the
//! long line is (docs/voice-plan.md, 「How a line crosses」).

use std::path::PathBuf;
use std::process::Command;

use super::mouth_long_line::voice_python;

#[test]
fn a_line_on_the_voice_server_writes_its_stages_counts_the_servers_own_room_and_is_dropped_once_its_asker_goes()
 {
  let python = voice_python();
  let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mouth_stages.py");
  let ran = Command::new(&python)
    .arg(&script)
    .output()
    .unwrap_or_else(|why| panic!("the Python tests' interpreter {} does not run: {why}", python.display()));

  assert!(
    ran.status.success(),
    "the voice server's account of a line is not as it should be:\n{}{}",
    String::from_utf8_lossy(&ran.stdout),
    String::from_utf8_lossy(&ran.stderr)
  );
}
