//! `bmo-voice` (`docs/boundary-api.md`, 「The command line」): the harness plugin's MCP server, the OpenAPI
//! document, and the health the deployment waits on.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use bmo_voice::client::VoiceClient;
use bmo_voice::settings::Settings;
use bmo_voice::wire::{BriefingKind, NewBriefing, NewTranscription, SynthesisState};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bmo-voice", version, about = "The voice engine's harness plugin and commands")]
struct Command {
  #[command(subcommand)]
  word: Word,
}

#[derive(Subcommand)]
enum Word {
  /// Answers the nouns this command carries: `listen` and `brief`.
  Nouns,
  /// Takes sound files down as text: the ear group's first-class verb, `bmo listen`.
  Listen {
    /// The sound files, each a path on this machine.
    #[arg(required = true)]
    files: Vec<String>,
    #[arg(long, default_value = "ko")]
    language: String,
    /// The longest wait for the transcription to end, in seconds.
    #[arg(long, default_value_t = 600)]
    wait: i64,
    #[arg(long)]
    json: bool,
  },
  /// Speaks a briefing for a room: the mouth group's first-class verb, `bmo brief`.
  Brief {
    /// The room the briefing is spoken for.
    #[arg(long)]
    chat: i64,
    /// The trained voice that speaks it.
    #[arg(long)]
    voice: String,
    /// What it says: the written words.
    #[arg(long)]
    text: String,
    /// The words the voice says, where the caller writes them; derived from the written words otherwise.
    #[arg(long)]
    spoken: Option<String>,
    /// `summary` for a line or two played by itself, `full` for a whole line read out.
    #[arg(long, default_value = "summary")]
    kind: String,
    /// What a listener shows in place of the words.
    #[arg(long)]
    title: Option<String>,
    /// One of the policy's emotions.
    #[arg(long)]
    emotion: Option<String>,
    /// The line a full reading speaks.
    #[arg(long)]
    message: Option<i64>,
    /// Whether a listener plays it by itself; the service's own answer where absent.
    #[arg(long)]
    plays: Option<bool>,
    /// The longest wait for the briefing to be spoken, in seconds.
    #[arg(long, default_value_t = 600)]
    wait: i64,
    #[arg(long)]
    json: bool,
  },
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
  /// Moves the old archive's voice tables from the item's export into the store (TODO #1923).
  Import {
    /// The export directory the item's export hand wrote.
    #[arg(long)]
    export: PathBuf,
    /// The PostgreSQL address of the voice's own database.
    #[arg(long)]
    database: String,
    /// Reads the export and reports what it would write, writing nothing.
    #[arg(long)]
    dry_run: bool,
  },
}

// How long the health is waited for before it counts as no answer.
const HEALTH_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

// How often a transcription or a briefing still going is looked at again.
const LOOK_AGAIN: Duration = Duration::from_millis(500);

// The service's client, at its own address and with its token as the settings name them.
fn connect(settings: &Settings) -> Result<VoiceClient, String> {
  let token = settings.token(&settings.token_file).map_err(|why| why.to_string())?;

  VoiceClient::new(&settings.address, token).map_err(|why| why.to_string())
}

// `bmo listen`: the files taken down in one language, waiting for the transcription to end.
async fn listen(
  settings: &Settings,
  files: Vec<String>,
  language: String,
  wait: i64,
  json: bool,
) -> Result<(), String> {
  let client = connect(settings)?;
  let mut held = client
    .create_transcription(&NewTranscription { language, files })
    .await
    .map_err(|why| why.to_string())?;
  let deadline = Instant::now() + Duration::from_secs(wait.max(1).unsigned_abs());

  while held.state == "waiting" || held.state == "running" {
    if Instant::now() >= deadline {
      return Err(format!("transcription {} did not end within {wait}s", held.id));
    }
    tokio::time::sleep(LOOK_AGAIN).await;
    held = client.transcription(held.id).await.map_err(|why| why.to_string())?;
  }

  if held.state != "done" {
    return Err(format!("transcription {} ended {}", held.id, held.state));
  }

  if json {
    println!("{}", serde_json::to_string(&held).map_err(|why| why.to_string())?);
  } else {
    println!("{}", held.text.unwrap_or_default());
  }

  Ok(())
}

// A briefing's two texts: the written words and, where the caller writes them, the spoken words.
struct Words {
  text: String,
  spoken: Option<String>,
}

// `bmo brief`: a line spoken for a room, waiting for the engine to speak it.
#[allow(clippy::too_many_arguments)]
async fn brief(
  settings: &Settings,
  chat: i64,
  voice: String,
  words: Words,
  kind: String,
  title: Option<String>,
  emotion: Option<String>,
  message: Option<i64>,
  plays: Option<bool>,
  wait: i64,
  json: bool,
) -> Result<(), String> {
  let Some(kind) = BriefingKind::parse(&kind) else {
    return Err(format!("{kind}: a briefing is summary or full"));
  };
  let client = connect(settings)?;
  let mut held = client
    .create_briefing(&NewBriefing {
      voice_name: voice,
      text: words.text,
      spoken: words.spoken,
      language: None,
      chat_id: chat,
      message_id: message,
      kind,
      title,
      emotion,
      plays,
      key: None,
    })
    .await
    .map_err(|why| why.to_string())?;
  let deadline = Instant::now() + Duration::from_secs(wait.max(1).unsigned_abs());

  while matches!(held.state, SynthesisState::Waiting | SynthesisState::Speaking) {
    if Instant::now() >= deadline {
      return Err(format!("briefing {} was not spoken within {wait}s", held.id));
    }
    tokio::time::sleep(LOOK_AGAIN).await;
    held = client.briefing(held.id).await.map_err(|why| why.to_string())?;
  }

  if held.state == SynthesisState::Failed {
    return Err(format!("briefing {} failed", held.id));
  }

  if json {
    println!("{}", serde_json::to_string(&held).map_err(|why| why.to_string())?);
  } else {
    println!("{}", held.id);
  }

  Ok(())
}

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

  // The nouns are answered before the settings are read, as every unit command's are.
  if let Word::Nouns = command.word {
    for noun in ["listen", "brief"] {
      println!("{noun}\t");
    }

    return ExitCode::SUCCESS;
  }

  if let Word::Openapi = command.word {
    let written = std::io::stdout().lock().write_all(bmo_voice::openapi::openapi_json().as_bytes());

    return if written.is_ok() { ExitCode::SUCCESS } else { ExitCode::FAILURE };
  }

  // The import reads its own arguments and never the deployment's settings.
  if let Word::Import {
    export,
    database,
    dry_run,
  } = &command.word
  {
    return match bmo_voice::import::run(database, export, *dry_run).await {
      Ok(report) => {
        report.lines().iter().for_each(|line| println!("{line}"));
        if report.sample.failures.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
      }
      Err(why) => {
        eprintln!("bmo-voice: {why}");
        ExitCode::FAILURE
      }
    };
  }

  let settings = match Settings::load() {
    Ok(settings) => settings,
    Err(why) => {
      eprintln!("bmo-voice: {why}");
      return ExitCode::FAILURE;
    }
  };
  let done = match command.word {
    Word::Nouns => unreachable!("the nouns are answered before the settings are read"),
    Word::Listen {
      files,
      language,
      wait,
      json,
    } => listen(&settings, files, language, wait, json).await,
    Word::Brief {
      chat,
      voice,
      text,
      spoken,
      kind,
      title,
      emotion,
      message,
      plays,
      wait,
      json,
    } => {
      let words = Words { text, spoken };

      brief(&settings, chat, voice, words, kind, title, emotion, message, plays, wait, json).await
    }
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
    Word::Import { .. } => unreachable!("the import is answered before the settings are read"),
  };

  match done {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("bmo-voice: {why}");
      ExitCode::FAILURE
    }
  }
}
