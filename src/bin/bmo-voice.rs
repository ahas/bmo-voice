//! `bmo-voice` (`docs/boundary-api.md`, 「The command line」): the ear's and the mouth's verbs, a document's
//! narration, the harness plugin's MCP server, the OpenAPI document, and the health the deployment waits on.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use bmo_voice::client::VoiceClient;
use bmo_voice::narration::{self, NARRATION_WAIT};
use bmo_voice::settings::Settings;
use bmo_voice::units::Caller;
use bmo_voice::units::artifact::{Artifact, NewAttachment};
use bmo_voice::wire::{BriefingKind, NewBriefing, NewSynthesis, NewTranscription, SynthesisState};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bmo-voice", version, about = "The voice engine's harness plugin and commands")]
struct Command {
  #[command(subcommand)]
  word: Word,
}

#[derive(Subcommand)]
enum Word {
  /// Answers the nouns this command carries: `listen`, `brief` and `brief-attach`.
  Nouns,
  /// Takes sound files down as text: the ear group's first-class verb, `bmo listen`.
  Listen {
    /// The sound files, each a path on this machine.
    #[arg(required = true)]
    files: Vec<String>,
    #[arg(long, default_value = "ko")]
    language: String,
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
    #[arg(long)]
    json: bool,
  },
  /// Speaks a text as a document's narration and attaches its sound to that document of the archive:
  /// `bmo brief-attach`.
  BriefAttach {
    /// The archive's number of the document.
    document: i64,
    /// The file holding the text to speak, the written words.
    file: PathBuf,
    /// The trained voice that speaks it; the settings' narration voice where absent.
    voice: Option<String>,
    /// How many minutes the synthesis is waited on at most before it is let go and the command ends failed.
    #[arg(long, default_value_t = NARRATION_WAIT.as_secs() / 60, value_parser = clap::value_parser!(u64).range(1..))]
    within: u64,
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

// How often a transcription or a briefing still going is looked at again.
const LOOK_AGAIN: Duration = Duration::from_millis(500);

// The service's client, at its own address and with its token as the settings name them.
fn connect(settings: &Settings) -> Result<VoiceClient, String> {
  let token = settings.token(&settings.token_file).map_err(|why| why.to_string())?;

  VoiceClient::new(&settings.address, token).map_err(|why| why.to_string())
}

// `bmo listen`: the files taken down in one language, waiting for the transcription to end however long it takes.
async fn listen(
  settings: &Settings,
  files: Vec<String>,
  language: String,
  json: bool,
) -> Result<(), String> {
  let client = connect(settings)?;
  let mut held = client
    .create_transcription(&NewTranscription { language, files })
    .await
    .map_err(|why| why.to_string())?;
  while held.state == "waiting" || held.state == "running" {
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

// `bmo brief`: a line spoken for a room, waiting for the engine to speak it however long it takes.
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
  while matches!(held.state, SynthesisState::Waiting | SynthesisState::Speaking) {
    tokio::time::sleep(LOOK_AGAIN).await;
    held = client.briefing(held.id).await.map_err(|why| why.to_string())?;
  }

  match held.state {
    SynthesisState::Failed => return Err(format!("briefing {} failed", held.id)),
    SynthesisState::Stopped => return Err(format!("briefing {} was stopped", held.id)),
    _ => {}
  }

  if json {
    println!("{}", serde_json::to_string(&held).map_err(|why| why.to_string())?);
  } else {
    println!("{}", held.id);
  }

  Ok(())
}

// The archive's number of a document and the file holding the words its narration says.
struct Narrated {
  document: i64,
  file: PathBuf,
}

// `bmo brief-attach`: a text spoken as a document's narration, its synthesis waited on within the bound, and its sound
// attached to the document as its head sound.
async fn brief_attach(
  settings: &Settings,
  narrated: Narrated,
  voice: Option<String>,
  within: Duration,
) -> Result<(), String> {
  let Some(voice) = voice.or_else(|| settings.narration.voice.clone()) else {
    return Err("no voice: name one, or the settings' [narration] voice".to_owned());
  };
  let Some(archive) = &settings.units.artifact else {
    return Err("the settings name no archive to attach to, [units.artifact]".to_owned());
  };
  let archive =
    Caller::new("artifact", settings, archive, reqwest::Client::new()).map_err(|why| why.to_string())?;
  let archive = Artifact::new(archive);
  let text =
    std::fs::read_to_string(&narrated.file).map_err(|why| format!("{}: {why}", narrated.file.display()))?;
  let client = connect(settings)?;
  let language = client.voice(&voice).await.map_err(|why| why.to_string())?.language;
  let taken = client
    .create_synthesis(&NewSynthesis {
      voice_name: voice,
      text: text.trim().to_owned(),
      spoken: None,
      language: None,
      emotion: None,
      style: None,
      seed: None,
      key: None,
    })
    .await
    .map_err(|why| why.to_string())?;
  eprintln!("synthesis {} taken in, waited on for at most {}", taken.id, minutes(within));
  let done = narration::wait(taken, within, |id| client.synthesis(id), |line| eprintln!("  {line}"))
    .await
    .map_err(|why| why.to_string())?;
  let sound = client.synthesis_sound(done.id, None).await.map_err(|why| why.to_string())?;
  let name = format!("narration-{language}.ogg");
  let kept = archive
    .attach(
      narrated.document,
      &NewAttachment {
        name: &name,
        media_type: "audio/ogg",
        role: Some("head"),
        content: &sound,
      },
    )
    .await
    .map_err(|why| why.to_string())?;

  println!(
    "attached {} ({} bytes, {} ms) to document {} as attachment {}",
    kept.name,
    sound.len(),
    done.duration_ms.unwrap_or_default(),
    narrated.document,
    kept.id
  );

  Ok(())
}

// A bound as the minutes it is named in.
fn minutes(within: Duration) -> String {
  match within.as_secs() / 60 {
    1 => "1 minute".to_owned(),
    count => format!("{count} minutes"),
  }
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
    for noun in ["listen", "brief", "brief-attach"] {
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
      json,
    } => listen(&settings, files, language, json).await,
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
      json,
    } => {
      let words = Words { text, spoken };

      brief(&settings, chat, voice, words, kind, title, emotion, message, plays, json).await
    }
    Word::BriefAttach {
      document,
      file,
      voice,
      within,
    } => {
      let within = Duration::from_secs(within.saturating_mul(60));

      brief_attach(&settings, Narrated { document, file }, voice, within).await
    }
    Word::Health => health(&settings).await,
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
