//! A finished synthesis's sound written to the file its caller names (docs/boundary-api.md, 「The hosted tool」):
//! the synthesis read for its key and its state, its bytes fetched through the engine's own client, and the file
//! replaced whole or left as it stood.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use bmo_voice::wire::{Synthesis, SynthesisState};

use crate::call::Answer;
use crate::servers::{client_of, failure_text};

/// The entry's full name.
pub const ENTRY: &str = "worker/voice_sound";

/// What the entry is given: the synthesis, the key it was taken in under and the file to write.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Asked {
  pub synthesis_id: i64,
  pub key: String,
  pub path: String,
}

/// The entry's arguments read, or why they do not: an id above zero, a key that holds something, and an absolute
/// path. The tool host checks the same schema before it runs the entry; this reads them again for a call that
/// comes another way.
pub fn read_sound_arguments(arguments: &serde_json::Value) -> Result<Asked, String> {
  let asked: Asked = serde_json::from_value(arguments.clone()).map_err(|why| {
    format!("{ENTRY} takes a synthesisId, the key it was taken in under and an absolute path: {why}")
  })?;

  if asked.synthesis_id < 1 {
    return Err(format!("{ENTRY} takes a synthesisId above zero, not {}", asked.synthesis_id));
  }

  if asked.key.is_empty() {
    return Err(format!("{ENTRY} takes the non-blank key the synthesis was taken in under"));
  }

  if !Path::new(&asked.path).is_absolute() {
    return Err(format!("{ENTRY} takes an absolute path to write, not {}", asked.path));
  }

  Ok(asked)
}

/// Why the synthesis at `synthesis` is not the caller's own to write, or none where it is: it was taken in under a
/// key, and that key is the one the caller gave.
pub fn not_its_own(synthesis: &Synthesis, key: &str) -> Option<String> {
  match synthesis.key.as_deref() {
    None => Some(format!(
      "synthesis {} was not taken in under a key, so it is not the caller's own; a line is taken in with its key \
       and the sound asked for under the same key",
      synthesis.id
    )),
    Some(own) if own != key => Some(format!(
      "synthesis {} was taken in under another key, so it is not the caller's own",
      synthesis.id
    )),
    Some(_) => None,
  }
}

/// Why the synthesis's sound does not stand yet, or none where it does: only a done synthesis has a sound.
pub fn not_finished(synthesis: &Synthesis) -> Option<String> {
  if synthesis.state == SynthesisState::Done {
    return None;
  }

  Some(format!(
    "synthesis {} is {}, and its sound stands only once it is done",
    synthesis.id,
    synthesis.state.as_str()
  ))
}

/// The sound of the caller's own finished synthesis at `asked`, written to the path it names, as the call's
/// answer.
pub async fn sound(settings: &Path, asked: &Asked) -> Answer {
  let client = match client_of(settings) {
    Ok(client) => client,
    Err(why) => return Answer::error(why),
  };
  let synthesis = match client.synthesis(asked.synthesis_id).await {
    Ok(synthesis) => synthesis,
    Err(failure) => return Answer::error(failure_text(&failure)),
  };

  if let Some(why) = not_its_own(&synthesis, &asked.key).or_else(|| not_finished(&synthesis)) {
    return Answer::error(why);
  }

  let bytes = match client.synthesis_sound(asked.synthesis_id, None).await {
    Ok(bytes) => bytes,
    Err(failure) => return Answer::error(failure_text(&failure)),
  };

  match write(&asked.path, &bytes) {
    Ok(()) => Answer::text(format!(
      "wrote synthesis {} to {} ({} bytes of OGG/Opus)",
      asked.synthesis_id,
      asked.path,
      bytes.len()
    )),
    Err(why) => Answer::error(why),
  }
}

/// `bytes` written at `path` whole or not at all: the file beside it and renamed over it, so a failure leaves any
/// file already there as it stood. The folders the path names are made where they do not stand.
pub fn write(path: &str, bytes: &[u8]) -> Result<(), String> {
  let target = Path::new(path);
  let parent = target
    .parent()
    .filter(|parent| !parent.as_os_str().is_empty())
    .ok_or_else(|| format!("{path} names no folder to write in"))?;
  let name = target.file_name().ok_or_else(|| format!("{path} names no file"))?;

  std::fs::create_dir_all(parent)
    .map_err(|why| format!("{path} was not written: its folder could not be made: {why}"))?;

  let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|since| since.as_nanos()).unwrap_or(0);
  let beside = parent.join(format!(".{}.part-{}-{stamp}", name.to_string_lossy(), std::process::id()));

  if let Err(why) = std::fs::write(&beside, bytes) {
    let _ = std::fs::remove_file(&beside);
    return Err(format!("{path} was not written: {why}"));
  }

  if let Err(why) = std::fs::rename(&beside, target) {
    let _ = std::fs::remove_file(&beside);
    return Err(format!("{path} was not written: {why}"));
  }

  Ok(())
}
