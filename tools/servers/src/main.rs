//! The voice's hosted tool's executable: one call handed on standard input, answered on standard output
//! (bmo-pocket's `docs/mcp-host.md`, 「A call」).

use std::process::ExitCode;

use voice_servers_tool::{answer, read_handed};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
  let said = match read_handed(std::io::stdin().lock()) {
    Ok(handed) => answer(&handed).await,
    Err(why) => Err(why),
  };
  let written = said
    .and_then(|said| serde_json::to_writer(std::io::stdout().lock(), &said).map_err(|why| why.to_string()));

  match written {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("{why}");
      ExitCode::FAILURE
    }
  }
}
