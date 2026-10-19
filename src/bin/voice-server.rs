//! The voice service (`docs/boundary-api.md`): reads its settings and serves the routes other units call.

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
  match bmo_voice::service::serve().await {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("voice-server: {why}");
      ExitCode::FAILURE
    }
  }
}
