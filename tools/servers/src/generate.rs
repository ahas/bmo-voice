//! A sound of any words in any served voice, written to a file (docs/boundary-api.md, 「The hosted tool」): the
//! words taken in as a synthesis, read until it ends within its bound, and its sound written whole. It is no
//! briefing, so it is tied to no room and no line, and no listener plays it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bmo_voice::client::{ClientFailure, VoiceClient};
use bmo_voice::wire::{NewSynthesis, PageQuery, Synthesis, SynthesisState};

use crate::call::Answer;
use crate::servers::{client_of, failure_text};
use crate::sound::write;

/// The entry's full name.
pub const ENTRY: &str = "voice/generate";

/// What the entry is given: the voice, the words, and optionally the emotion, the language and the file to write.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Asked {
  pub voice_name: String,
  pub text: String,
  #[serde(default)]
  pub emotion: Option<String>,
  #[serde(default)]
  pub language: Option<String>,
  #[serde(default)]
  pub path: Option<String>,
}

/// How often the synthesis is read while it has not ended, and how long it is waited for before it is let go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pace {
  pub every: Duration,
  pub within: Duration,
}

impl Default for Pace {
  /// Every half second, and at most ten minutes.
  fn default() -> Self {
    Self {
      every: Duration::from_millis(500),
      within: Duration::from_secs(600),
    }
  }
}

/// The entry's arguments read, or why they do not: a voice and words that hold something, and an absolute path
/// where one is given. The tool host checks the same schema before it runs the entry; this reads them again for a
/// call that comes another way.
pub fn read_generate_arguments(arguments: &serde_json::Value) -> Result<Asked, String> {
  let asked: Asked = serde_json::from_value(arguments.clone())
    .map_err(|why| format!("{ENTRY} takes a voiceName and the text to say: {why}"))?;

  if asked.voice_name.trim().is_empty() {
    return Err(format!("{ENTRY} takes the name of the voice to speak in"));
  }

  if asked.text.trim().is_empty() {
    return Err(format!("{ENTRY} takes a text that holds something to say"));
  }

  if let Some(path) = asked.path.as_deref()
    && !Path::new(path).is_absolute()
  {
    return Err(format!("{ENTRY} takes an absolute path to write, not {path}"));
  }

  Ok(asked)
}

/// Where the sound of the synthesis is written: the path the caller named, else a file named for the synthesis
/// under the machine's temporary folder.
pub fn target(asked: &Asked, synthesis_id: i64) -> PathBuf {
  asked.path.as_deref().map_or_else(
    || std::env::temp_dir().join("bmo-voice").join(format!("generated-{synthesis_id}.ogg")),
    PathBuf::from,
  )
}

/// Why an ended synthesis has no sound to write, or none where it is done.
pub fn not_spoken(synthesis: &Synthesis) -> Option<String> {
  match synthesis.state {
    SynthesisState::Done => None,
    SynthesisState::Failed | SynthesisState::Stopped => Some(format!(
      "synthesis {} {}: {}",
      synthesis.id,
      synthesis.state.as_str(),
      synthesis.failure.as_deref().unwrap_or("its reason was not kept")
    )),
    SynthesisState::Waiting | SynthesisState::Speaking => {
      Some(format!("synthesis {} is {} and has not ended", synthesis.id, synthesis.state.as_str()))
    }
  }
}

/// What the call answers once the sound is written: the path, the bytes, the length, the voice, the synthesis and
/// the words the voice said.
pub fn written_text(path: &Path, bytes: usize, synthesis: &Synthesis) -> String {
  let seconds = synthesis.duration_ms.map_or(0.0, |ms| ms as f64 / 1000.0);
  let mut text = format!(
    "wrote {}: {bytes} bytes of OGG/Opus, {seconds:.1} seconds in the voice {}, synthesis {}",
    path.display(),
    synthesis.voice_name,
    synthesis.id
  );

  if let Some(spoken) = synthesis.spoken.as_deref() {
    text.push_str(&format!("\nThe voice said: {spoken}"));
  }

  text
}

// Whether a refusal says the voice named stands in no list.
fn no_such_voice(failure: &ClientFailure) -> bool {
  matches!(failure, ClientFailure::Answered { status: 404, answer: Some(answer), .. } if answer.code == "not-found")
}

// The names of every served voice, page by page, or why they could not be read.
async fn served_voices(client: &VoiceClient) -> Result<Vec<String>, ClientFailure> {
  let mut names = Vec::new();
  let mut after = None;

  loop {
    let page = client
      .voices(&PageQuery {
        after: after.clone(),
        limit: Some(1000),
      })
      .await?;

    names.extend(page.items.iter().filter(|voice| voice.served).map(|voice| voice.name.clone()));

    match (page.more, page.items.last()) {
      (true, Some(last)) => after = Some(last.name.clone()),
      _ => return Ok(names),
    }
  }
}

// The refusal of a voice no list holds, naming the voices that are served where they read.
async fn unknown_voice(client: &VoiceClient, voice_name: &str) -> String {
  match served_voices(client).await {
    Ok(names) if names.is_empty() => format!("no voice is named {voice_name}, and no voice is served"),
    Ok(names) => format!("no voice is named {voice_name}; the served voices are {}", names.join(", ")),
    Err(failure) => format!(
      "no voice is named {voice_name}, and the served voices could not be read: {}",
      failure_text(&failure)
    ),
  }
}

// The synthesis read until it ends, or as it stood when the bound ran out.
async fn ended(client: &VoiceClient, taken: Synthesis, pace: Pace) -> Result<Synthesis, ClientFailure> {
  let started = tokio::time::Instant::now();
  let mut standing = taken;

  loop {
    if !matches!(standing.state, SynthesisState::Waiting | SynthesisState::Speaking)
      || started.elapsed() >= pace.within
    {
      return Ok(standing);
    }

    tokio::time::sleep(pace.every).await;
    standing = client.synthesis(standing.id).await?;
  }
}

/// A sound of the words asked, written where the caller names or under the temporary folder, as the call's answer.
pub async fn generate(settings: &Path, asked: &Asked) -> Answer {
  generate_at(settings, asked, Pace::default()).await
}

/// [`generate`] at the pace given.
pub async fn generate_at(settings: &Path, asked: &Asked, pace: Pace) -> Answer {
  let client = match client_of(settings) {
    Ok(client) => client,
    Err(why) => return Answer::error(why),
  };
  let taken = client
    .create_synthesis(&NewSynthesis {
      voice_name: asked.voice_name.clone(),
      text: asked.text.clone(),
      spoken: None,
      language: asked.language.clone(),
      emotion: asked.emotion.clone(),
      style: None,
      seed: None,
      key: None,
      asked_by: None,
    })
    .await;
  let taken = match taken {
    Ok(taken) => taken,
    Err(failure) if no_such_voice(&failure) => {
      return Answer::error(unknown_voice(&client, &asked.voice_name).await);
    }
    Err(failure) => return Answer::error(failure_text(&failure)),
  };
  let synthesis = match ended(&client, taken, pace).await {
    Ok(synthesis) => synthesis,
    Err(failure) => return Answer::error(failure_text(&failure)),
  };

  if let Some(why) = not_spoken(&synthesis) {
    let waits = synthesis
      .waiting_on
      .as_deref()
      .map(|on| format!(", waiting on: {on}"))
      .unwrap_or_default();

    return Answer::error(format!("{why}{waits}"));
  }

  let bytes = match client.synthesis_sound(synthesis.id, None).await {
    Ok(bytes) => bytes,
    Err(failure) => return Answer::error(failure_text(&failure)),
  };
  let path = target(asked, synthesis.id);

  match write(&path.to_string_lossy(), &bytes) {
    Ok(()) => Answer::text(written_text(&path, bytes.len(), &synthesis)),
    Err(why) => Answer::error(why),
  }
}
