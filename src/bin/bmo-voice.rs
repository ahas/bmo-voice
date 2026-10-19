//! The voice's unit command (`docs/boundary-api.md`, 「The unit command」): the commands `bmo` hands the voice.

use std::process::ExitCode;

fn main() -> ExitCode {
  eprintln!("bmo-voice: no command is served yet");
  ExitCode::from(2)
}
