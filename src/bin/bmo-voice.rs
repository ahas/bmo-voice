//! The voice's unit command (`docs/boundary-api.md`, 「The unit command」): `voice say`, `voice brief` and
//! `runner voice`, the commands `bmo` hands the voice. Asked `nouns`, it prints them.

use std::io::Read as _;
use std::path::PathBuf;
use std::process::ExitCode;

use bmo_voice::briefing::{self, Launch, hand, runner};
use bmo_voice::context::Context;
use bmo_voice::settings::Settings;
use clap::{Parser, Subcommand};

// The nouns `bmo` dispatches here, one a line.
const NOUNS: [&str; 3] = ["voice say", "voice brief", "runner voice"];

#[derive(Parser)]
#[command(name = "bmo-voice", about = "The voice's commands.")]
struct Words {
  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand)]
enum Command {
  /// Says a line out loud in a room, holding the caller until it is sent, or sends a sound file it is handed.
  #[command(name = "voice-say", alias = "voice", allow_negative_numbers = true)]
  Say {
    #[arg(long, allow_hyphen_values = true)]
    text: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    chat: Option<i64>,
    #[arg(long, allow_hyphen_values = true)]
    caption: Option<String>,
    #[arg(long)]
    file: Option<PathBuf>,
    /// The voice to speak in by its name in the voices table, where it is not the one the agent's registration
    /// names.
    #[arg(long, allow_hyphen_values = true)]
    voice: Option<String>,
  },
  /// Says a briefing out loud in a room without holding up whoever asked; the words come in on standard input where
  /// --text names none.
  #[command(name = "voice-brief", allow_negative_numbers = true)]
  Brief {
    #[arg(long, allow_hyphen_values = true)]
    chat: i64,
    /// Who set it off: `chat:<chatId>` for a session, or the launcher's own name.
    #[arg(long, allow_hyphen_values = true)]
    by: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    text: Option<String>,
    /// The emotion the line is said with, which the listeners showing a face are told.
    #[arg(long)]
    emotion: Option<String>,
    /// What a summary is about, in one line; a launch naming one is a summary.
    #[arg(long, allow_hyphen_values = true)]
    title: Option<String>,
    /// The document a narration is kept with; a launch naming one is that document's sound.
    #[arg(long)]
    document: Option<i64>,
    /// The road the turn that launched it answers on.
    #[arg(long)]
    via: Option<String>,
  },
  /// The run of a briefing its launcher started, which speaks the line and puts it where it belongs.
  #[command(name = "voice-runner", allow_negative_numbers = true, hide = true)]
  Runner {
    chat_id: i64,
    text_path: PathBuf,
    run_id: Option<i64>,
    unit: Option<String>,
    /// The message the voice answers.
    #[arg(long)]
    reply_to: Option<i64>,
    /// The line the voice speaks, where it voices a line already standing in the room.
    #[arg(long)]
    voices: Option<i64>,
    /// The reading is not written out under the voice.
    #[arg(long)]
    no_transcript: bool,
    #[arg(long)]
    emotion: Option<String>,
    #[arg(long)]
    summary: bool,
    /// A line's full reading, kept on its line rather than sent.
    #[arg(long)]
    keep: bool,
    /// The document a narration is kept as the sound of.
    #[arg(long)]
    document: Option<i64>,
    /// The agent whose voice speaks the line.
    #[arg(long, allow_hyphen_values = true)]
    agent: Option<String>,
    /// The road the turn that launched it answers on.
    #[arg(long)]
    via: Option<String>,
  },
  /// Whether the service the settings name answers its health: 0 where it does, 1 where it does not.
  Health,
}

// How long the health is waited for before it counts as no answer.
const HEALTH_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

// The service's `GET /health`, at its own address as the settings name it.
async fn health() -> Result<String, (u8, String)> {
  let settings = Settings::load().map_err(|why| (1, format!("voice: {why}")))?;
  let url = format!("{}/health", settings.address.trim_end_matches('/'));
  let answer = reqwest::Client::new()
    .get(&url)
    .timeout(HEALTH_WAIT)
    .send()
    .await
    .map_err(|why| (1, format!("voice: {url} did not answer: {why}")))?;

  if !answer.status().is_success() {
    return Err((1, format!("voice: {url} answered {}", answer.status().as_u16())));
  }

  let said: serde_json::Value = answer
    .json()
    .await
    .map_err(|why| (1, format!("voice: {url} answered no health: {why}")))?;

  if said["ok"] != true {
    return Err((1, format!("voice: {url} answered {said}")));
  }

  Ok(String::new())
}

// The words as `bmo` hands them, a two-word noun joined into the one command name it stands for.
fn joined(mut words: Vec<String>) -> Vec<String> {
  let named = match (words.get(1).map(String::as_str), words.get(2).map(String::as_str)) {
    (Some("voice"), Some("say")) => Some("voice-say"),
    (Some("voice"), Some("brief")) => Some("voice-brief"),
    (Some("runner"), Some("voice")) => Some("voice-runner"),
    _ => None,
  };

  if let Some(named) = named {
    words.splice(1..3, [named.to_owned()]);
  }

  words
}

fn ended(answer: Result<String, (u8, String)>) -> ExitCode {
  match answer {
    Ok(said) => {
      if !said.is_empty() {
        println!("{said}");
      }

      ExitCode::SUCCESS
    }
    Err((code, said)) => {
      eprintln!("{said}");
      ExitCode::from(code)
    }
  }
}

#[tokio::main]
async fn main() -> ExitCode {
  let words: Vec<String> = std::env::args().collect();

  if words.get(1).map(String::as_str) == Some("nouns") {
    for noun in NOUNS {
      println!("{noun}");
    }

    return ExitCode::SUCCESS;
  }

  let words = match Words::try_parse_from(joined(words)) {
    Ok(words) => words,
    Err(error) => {
      let _ = error.print();

      return ExitCode::from(if error.use_stderr() { 2 } else { 0 });
    }
  };
  // The health asks the running service and opens no store of its own.
  if matches!(words.command, Command::Health) {
    return ended(health().await);
  }

  let context = match Context::open(4).await {
    Ok(context) => context,
    Err(why) => return ended(Err((1, format!("voice: {why}")))),
  };

  ended(match words.command {
    Command::Say {
      text,
      chat,
      caption,
      file,
      voice,
    } => {
      let asked = hand::Asked {
        text: text.unwrap_or_default(),
        chat,
        caption,
        file,
        voice,
      };

      hand::voice(&context, asked).await.map_err(|said| (1, said))
    }
    Command::Brief {
      chat,
      by,
      text,
      emotion,
      title,
      document,
      via,
    } => {
      // The words come on standard input by default: a briefing can be long, and an argument stands in the process
      // list, where a line of the agent's own speech has no business being.
      let text = text.unwrap_or_else(|| {
        let mut read = String::new();
        let _ = std::io::stdin().read_to_string(&mut read);

        read
      });
      let launch = Launch {
        chat_id: chat,
        text,
        agent: std::env::var("BMO_TURN_AGENT").ok().filter(|agent| !agent.is_empty()),
        started_by: by.unwrap_or_default(),
        summary: title.is_some(),
        title: title.map(|title| title.trim().to_owned()),
        document,
        emotion,
        via,
        ..Launch::default()
      };

      match briefing::launch(&context, &launch).await {
        Ok(Ok(launched)) => Ok(match launch.document {
          Some(id) => format!(
            "narration of document {id} launched as {}; it is kept as the document's sound when it is done and goes to no room.",
            launched.unit
          ),
          None => briefing::launched(&launched.unit, chat),
        }),
        Ok(Err(refused)) => Err((1, format!("voice: {refused}"))),
        Err(why) => Err((1, format!("voice: {why}"))),
      }
    }
    Command::Runner {
      chat_id,
      text_path,
      run_id,
      unit,
      reply_to,
      voices,
      no_transcript,
      emotion,
      summary,
      keep,
      document,
      agent,
      via: _,
    } => {
      let started = runner::Started {
        chat_id,
        text_path,
        run_id,
        unit: unit.unwrap_or_default(),
        reply_to,
        voices,
        transcript: !no_transcript,
        emotion,
        summary,
        keep,
        document,
        agent,
      };

      runner::voice_runner(&context, started)
        .await
        .map_err(|(code, said)| (u8::try_from(code).unwrap_or(1), said))
    }
    Command::Health => health().await,
  })
}
