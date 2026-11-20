//! Where the transcriber hears (`tests/transcriber_card.py`), run on the Python tests' own interpreter as the mouth's
//! tests are (docs/voice-plan.md, 「Where a transcription runs」).

use std::path::PathBuf;
use std::process::Command;

use super::mouth_long_line::voice_python;

#[test]
fn a_card_held_or_out_of_memory_hands_the_transcription_to_the_processor() {
  let python = voice_python();
  let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/transcriber_card.py");
  let ran = Command::new(&python)
    .arg(&script)
    .output()
    .unwrap_or_else(|why| panic!("the Python tests' interpreter {} does not run: {why}", python.display()));

  assert!(
    ran.status.success(),
    "the transcriber did not hear where it should:\n{}{}",
    String::from_utf8_lossy(&ran.stdout),
    String::from_utf8_lossy(&ran.stderr)
  );
}
