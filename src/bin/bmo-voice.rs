//! `bmo-voice` (`docs/boundary-api.md`, 「The command line」): the harness plugin's MCP server, the OpenAPI
//! document, and the health the deployment waits on.

use std::io::Write;
use std::process::ExitCode;

use bmo_voice::client::VoiceClient;
use bmo_voice::settings::Settings;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bmo-voice", version, about = "The voice engine's harness plugin and commands")]
struct Command {
  #[command(subcommand)]
  word: Word,
}

#[derive(Subcommand)]
enum Word {
  /// Whether the service the settings name answers ready; ends 0 where it does.
  Health,
  /// Runs the harness plugin's MCP server on standard input and output, calling the service at its address.
  Mcp {
    /// The service's base address; the settings' address where absent.
    #[arg(long)]
    address: Option<String>,
    /// The service's bearer token; the settings' token file where absent.
    #[arg(long)]
    token: Option<String>,
  },
  /// Prints the OpenAPI document, as `docs/openapi.json` keeps it.
  Openapi,
}

// How long the health is waited for before it counts as no answer.
const HEALTH_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

// The service's `GET /health`, at its own address and with its token as the settings name them.
async fn health(settings: &Settings) -> Result<(), String> {
  let token = settings.token(&settings.token_file).map_err(|why| why.to_string())?;
  let client = VoiceClient::new(&settings.address, token).map_err(|why| why.to_string())?;
  let answer = client.health().await.map_err(|why| why.to_string())?;

  if answer.ready {
    Ok(())
  } else {
    Err(answer.reason.unwrap_or_else(|| "the service is not ready".to_owned()))
  }
}

#[tokio::main]
async fn main() -> ExitCode {
  let command = Command::parse();

  if let Word::Openapi = command.word {
    let written = std::io::stdout().lock().write_all(bmo_voice::openapi::openapi_json().as_bytes());

    return if written.is_ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
  }

  let settings = match Settings::load() {
    Ok(settings) => settings,
    Err(why) => {
      eprintln!("bmo-voice: {why}");
      return ExitCode::FAILURE;
    }
  };
  let done = match command.word {
    Word::Health => {
      let waited = tokio::time::timeout(HEALTH_WAIT, health(&settings)).await;

      match waited {
        Ok(answer) => answer,
        Err(_) => Err(format!("{} did not answer within {}s", settings.address, HEALTH_WAIT.as_secs())),
      }
    }
    Word::Mcp { address, token } => {
      let address = address.unwrap_or_else(|| settings.address.clone());
      let token = match token {
        Some(token) => Some(token),
        None => match settings.token(&settings.token_file) {
          Ok(token) => token,
          Err(why) => {
            eprintln!("bmo-voice: {why}");
            return ExitCode::FAILURE;
          }
        },
      };

      match VoiceClient::new(&address, token) {
        Ok(client) => bmo_voice::mcp::serve(client).await,
        Err(why) => Err(why.to_string()),
      }
    }
    Word::Openapi => Ok(()),
  };

  match done {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("bmo-voice: {why}");
      ExitCode::FAILURE
    }
  }
}
