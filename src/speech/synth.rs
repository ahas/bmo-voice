//! What the engine runs on this machine (docs/voice-plan.md, 「One voice environment」, 「One at a time on the
//! operations machine's card」, 「The voice as a tool」): the voice's mouth, encoder and transcriber run through
//! its own tool entry, the line in front of the card every call to the synthesiser stands in, and how long a
//! spoken sound runs.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::settings::Settings;
use crate::store::voices::VoiceRow;

/// How long the mouth may take, its stand in the card's line included, and how long the encoder and the
/// transcriber may.
const MOUTH_TIMEOUT: Duration = Duration::from_secs(600);
const ENCODER_TIMEOUT: Duration = Duration::from_secs(60);
const TRANSCRIBER_TIMEOUT: Duration = Duration::from_secs(600);

/// The voice's tool entry in the synth folder, which runs the mouth and the encoder.
const TOOL_ENTRY: &str = "voice-tool.py";

/// How the mouth's refusal begins when the wait ran out and the line was never spoken.
const NO_TURN: &str = "no turn:";

/// The pause after one line before the next may begin.
pub const SUMMARY_PAUSE_MS: i64 = 600;

/// The one lock file in front of this machine's card: one for the machine rather than one per room, since what is
/// shared is the card.
pub fn lock_path(settings: &Settings) -> PathBuf {
  settings.locks_folder().join("voice-synth.lock")
}

/// The lock file there, made by whichever run comes first.
pub fn ready_lock(path: &Path) -> std::io::Result<()> {
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent)?;
  }

  std::fs::write(path, "")
}

/// Whether the card is busy this instant. Nothing is ordered by this — the lock in front of the synthesiser
/// orders the runs — and a run asks it only so its log can say it stood in line.
pub fn queue_busy(path: &Path) -> bool {
  let Ok(file) = std::fs::File::open(path) else { return false };

  matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

/// The words a line is handed to the mouth as: a file holding them, or the line itself.
pub enum Words<'a> {
  File(&'a Path),
  Line(&'a str),
}

/// Why the voice's entry did not do what it was called for.
#[derive(Debug, PartialEq, Eq)]
pub enum Unspoken {
  /// The turn on the card never came: another line held it past the wait, and the synthesiser was never reached.
  NoTurn,
  /// The entry failed, or the tool host did not answer: the reason as it was given.
  Failed(String),
}

/// What the mouth is called with: the words, where the sound goes, and the voice's row.
pub fn mouth_arguments(words: &Words<'_>, out: &Path, voice: &VoiceRow) -> Value {
  let mut arguments = json!({
    "out": out,
    "checkpoint": voice.checkpoint,
    "reference": voice.reference,
    "cfg": voice.cfg,
    "steps": voice.steps,
    "seed": voice.seed,
    "sentences": voice.by_sentence,
    "prompts": voice
      .prompts
      .iter()
      .map(|(language, prompt)| json!({ "language": language, "clip": prompt.clip, "words": prompt.words }))
      .collect::<Vec<_>>(),
  });

  match words {
    Words::File(file) => arguments["text_file"] = json!(file),
    Words::Line(text) => arguments["text"] = json!(text),
  }

  if !voice.style.is_empty() {
    arguments["style"] = json!(voice.style);
  }

  arguments
}

// One entry of the voice's own tool entry run to its end, handed the call as the tool host hands it: the entry,
// its arguments, and a record naming the data root, under whose voice folder the card's lock stands. Dropping the
// call before it answers ends the entry's process and whatever it runs, the synthesiser and its hold on the lock
// with it.
async fn call_entry(
  settings: &Settings,
  entry: &str,
  arguments: Value,
  timeout: Duration,
) -> Result<Value, String> {
  let folder = settings.synth_folder().map_err(|why| why.to_string())?;
  let handed = json!({
    "protocol": 1,
    "kind": "call",
    "entry": entry,
    "arguments": arguments,
    "session": { "line": "voice", "capabilities": [], "dataRoot": settings.data_root },
  });
  let mut child = tokio::process::Command::new("python3")
    .arg(folder.join(TOOL_ENTRY))
    .current_dir(&folder)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .process_group(0)
    .spawn()
    .map_err(|why| format!("the voice's tool entry could not be started: {why}"))?;

  if let Some(mut stdin) = child.stdin.take() {
    stdin
      .write_all(handed.to_string().as_bytes())
      .await
      .map_err(|why| format!("the voice's tool entry took no call: {why}"))?;
  }

  let done = tokio::time::timeout(timeout, child.wait_with_output())
    .await
    .map_err(|_| format!("the voice's {entry} entry did not answer within {}s", timeout.as_secs()))?
    .map_err(|why| format!("the voice's tool entry failed: {why}"))?;

  serde_json::from_slice(&done.stdout).map_err(|_| {
    let stderr = String::from_utf8_lossy(&done.stderr);
    let tail = stderr.trim().lines().last().unwrap_or_default().to_owned();

    format!("the voice's {entry} entry gave no answer (exit {:?}): {tail}", done.status.code())
  })
}

/// One line spoken on this machine's card by the voice's `voice/mouth` entry, which stands it in the card's line.
pub async fn speak(
  words: &Words<'_>,
  out: &Path,
  voice: &VoiceRow,
  settings: &Settings,
) -> Result<(), Unspoken> {
  answered(call_entry(settings, "voice/mouth", mouth_arguments(words, out, voice), MOUTH_TIMEOUT).await)
}

/// A wav a node spoke written out as OGG/Opus by the voice's `voice/encoder` entry. It stands in no line, since no
/// model is loaded and the card is not touched.
pub async fn encode(wav: &Path, out: &Path, settings: &Settings) -> Result<(), String> {
  let called =
    call_entry(settings, "voice/encoder", json!({ "from_wav": wav, "out": out }), ENCODER_TIMEOUT).await;

  answered(called).map_err(|unspoken| match unspoken {
    Unspoken::NoTurn => "the encoder never had its turn".to_owned(),
    Unspoken::Failed(why) => why,
  })
}

// An answer's text, a piece a line.
fn said(answer: &Value) -> Vec<String> {
  answer["content"]
    .as_array()
    .into_iter()
    .flatten()
    .map(|piece| piece["text"].as_str().map_or_else(|| piece.to_string(), str::to_owned))
    .collect()
}

// What a call of the voice's entries came to: its refusal for want of a turn told apart from every other failure.
fn answered(called: Result<Value, String>) -> Result<(), Unspoken> {
  let answer = called.map_err(Unspoken::Failed)?;

  if answer["isError"] != true {
    return Ok(());
  }

  let text = said(&answer).join("\n");

  if text.starts_with(NO_TURN) {
    return Err(Unspoken::NoTurn);
  }

  Err(Unspoken::Failed(text.trim_start_matches("ERROR: ").to_owned()))
}

/// How long an Ogg/Opus sound runs, its pages read by the ogg crate: the last packet's granule position counts 48 kHz
/// samples, less the pre-skip its first packet, the Opus header, names. None for anything that is not one.
pub fn ogg_opus_seconds(bytes: &[u8]) -> Option<f64> {
  let mut packets = ogg::PacketReader::new(std::io::Cursor::new(bytes));
  let head = packets.read_packet().ok()??;

  // The Opus header is the codec's own: its magic, its version, its channel count and then the pre-skip as a
  // little-endian 16-bit count (RFC 7845, section 5.1), which no maintained crate reads apart from a whole decoder.
  if head.data.len() < 12 || &head.data[..8] != b"OpusHead" {
    return None;
  }

  let pre_skip = u16::from_le_bytes([head.data[10], head.data[11]]);
  let mut granule = head.absgp_page();

  while let Ok(Some(packet)) = packets.read_packet() {
    granule = packet.absgp_page();
  }

  let granule = i64::try_from(granule).ok()?;

  Some((granule - i64::from(pre_skip)).max(0) as f64 / 48_000.0)
}

/// Spoken files taken down in one language: what was heard, each file's words in the order given, one to a line.
pub async fn transcribe(language: &str, files: &[String], settings: &Settings) -> Result<String, String> {
  let answer = call_entry(
    settings,
    "voice/transcriber",
    json!({ "language": language, "files": files }),
    TRANSCRIBER_TIMEOUT,
  )
  .await?;

  if answer["isError"] == true {
    return Err(said(&answer).join("\n").trim_start_matches("ERROR: ").to_owned());
  }

  let heard = answer["structuredContent"]["heard"]
    .as_array()
    .into_iter()
    .flatten()
    .filter_map(|one| one["heard"].as_str())
    .map(str::to_owned)
    .collect::<Vec<_>>();

  Ok(heard.join("\n"))
}
