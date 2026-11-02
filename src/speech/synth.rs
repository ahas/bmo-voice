//! What the engine runs on this machine (docs/voice-plan.md, 「The environments」, 「One at a time on the
//! operations machine's card」, 「The voice as a tool」): the voice's mouth, encoder and transcriber run through
//! its own tool entry, the line in front of the card every call to the synthesiser stands in, and how long a
//! spoken sound runs.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::settings::Settings;
use crate::store::voices::VoiceRow;

/// The voice's tool entry in the synth folder, which runs the mouth and the encoder.
const TOOL_ENTRY: &str = "voice-tool.py";

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

/// What the mouth is called with: the words, where the sound goes, and the voice's row.
pub fn mouth_arguments(words: &Words<'_>, out: &Path, voice: &VoiceRow) -> Value {
  let mut arguments = json!({
    "out": out,
    "checkpoint": voice.checkpoint,
    "seed": voice.seed,
    "sentences": voice.by_sentence,
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

// One entry of the voice's own tool entry run to its end, however long that takes, handed the call as the tool host
// hands it: the entry, its arguments, and a record naming the data root, under whose voice folder the card's lock
// stands. Dropping the call before it answers ends the entry's process and whatever it runs, the synthesiser and its
// hold on the lock with it.
async fn call_entry(settings: &Settings, entry: &str, arguments: Value) -> Result<Value, String> {
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

  let done = child
    .wait_with_output()
    .await
    .map_err(|why| format!("the voice's tool entry failed: {why}"))?;

  serde_json::from_slice(&done.stdout).map_err(|_| {
    let stderr = String::from_utf8_lossy(&done.stderr);
    let tail = stderr.trim().lines().last().unwrap_or_default().to_owned();

    format!("the voice's {entry} entry gave no answer (exit {:?}): {tail}", done.status.code())
  })
}

/// One line spoken on this machine's card by the voice's `voice/mouth` entry, which stands it in the card's line for
/// as long as the lines ahead of it take.
pub async fn speak(
  words: &Words<'_>,
  out: &Path,
  voice: &VoiceRow,
  settings: &Settings,
) -> Result<(), String> {
  answered(call_entry(settings, "voice/mouth", mouth_arguments(words, out, voice)).await)
}

/// A wav a node spoke written out as OGG/Opus by the voice's `voice/encoder` entry. It stands in no line, since no
/// model is loaded and the card is not touched.
pub async fn encode(wav: &Path, out: &Path, settings: &Settings) -> Result<(), String> {
  answered(call_entry(settings, "voice/encoder", json!({ "from_wav": wav, "out": out })).await)
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

// What a call of the voice's entries came to: done, or its failure as the entry gave it.
fn answered(called: Result<Value, String>) -> Result<(), String> {
  let answer = called?;

  if answer["isError"] != true {
    return Ok(());
  }

  Err(said(&answer).join("\n").trim_start_matches("ERROR: ").to_owned())
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
  let answer =
    call_entry(settings, "voice/transcriber", json!({ "language": language, "files": files })).await?;

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
