//! The voice service (`docs/boundary-api.md`): reads its settings and serves the routes other units call.

use std::process::ExitCode;

use bmo_voice::settings::Settings;

fn main() -> ExitCode {
  match Settings::load() {
    Ok(settings) => {
      eprintln!("voice-server: no route is served yet; it would listen on {}", settings.listen);
      ExitCode::from(2)
    }
    Err(why) => {
      eprintln!("voice-server: the settings do not read: {why}");
      ExitCode::FAILURE
    }
  }
}
