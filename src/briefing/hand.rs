//! `voice say`, the hand tool (`docs/voice-briefing.md`, 「The briefing」): a line said out loud in a room while its
//! caller waits — a person at a shell, or a script that already has the sound and wants it sent before it goes on.
//! It is never the way a session speaks; `voice brief` is.
//!
//! The words pass the reading gate as a briefing's do and are spoken in the voice the settings' agent's
//! registration names, or the one `--voice` names, on a free voice node or this machine's card in the line every
//! call to it stands in. A caller that already has the sound hands it over with `--file` and nothing is
//! synthesised.

use std::path::{Path, PathBuf};

use super::runner::clean;
use super::{switched_off, voice_of, voiceless};
use crate::context::Context;
use crate::now_ms;
use crate::policy::POLICY;
use crate::script::{GateRequest, reading};
use crate::speech::node::{self, Speech};
use crate::speech::synth::{self, QUEUE_WAIT_MS, Unspoken, Words};
use crate::store::voices::{self, VoiceRow};

/// How the hand tool is called.
pub const USAGE: &str = "usage: bmo-voice voice say --text '<words>' [--chat <chatId>] [--caption <text>] [--file <path.ogg>] [--voice <name>]";

/// What the hand tool was asked: the words, the room, a caption of its own, a sound file to send as it stands, and
/// the voice by name.
#[derive(Clone, Debug, Default)]
pub struct Asked {
  pub text: String,
  pub chat: Option<i64>,
  pub caption: Option<String>,
  pub file: Option<PathBuf>,
  pub voice: Option<String>,
}

fn said(why: impl std::fmt::Display) -> String {
  format!("voice: {why}")
}

/// The line said in the room, answered once its voice message is there.
pub async fn voice(context: &Context, asked: Asked) -> Result<String, String> {
  if asked.text.trim().is_empty() {
    return Err(USAGE.to_owned());
  }

  let chat_id = match asked.chat {
    Some(chat) => chat,
    None => context.units.receiver.owner().await.map_err(said)?.chat,
  };

  // The switch that turns the whole voice briefing off turns this road into a room off with it.
  if let Some(refusal) = switched_off(context).await.map_err(said)? {
    return Err(said(refusal));
  }

  // A line of the agent's whose registration names no voice is never spoken; a voice named by hand is a person
  // trying it.
  if asked.voice.is_none()
    && let Some(refusal) = voiceless(context, &context.settings.agent).await.map_err(said)?
  {
    return Err(said(refusal));
  }

  let words = clean(context, &asked.text, Some(chat_id)).await;
  let (path, made) = match &asked.file {
    Some(file) => (file.clone(), false),
    None => (spoken(context, &asked, &words, chat_id).await?, true),
  };
  let caption = match &asked.caption {
    Some(caption) => {
      let label = "the caption of a voice message the agent is about to send to a chat room";

      Some(
        context
          .units
          .gate
          .sanitised(caption, label, Some(chat_id))
          .await
          .unwrap_or_else(|_| caption.clone()),
      )
    }
    None => None,
  };
  let sent = context.units.receiver.send_file(chat_id, &path, caption.as_deref(), None).await;
  let bytes = std::fs::metadata(&path).map_or(0, |meta| meta.len());

  if made {
    let _ = std::fs::remove_file(&path);
  }

  let sent = sent.map_err(said)?;

  if let Some(failed) = sent.failed.first() {
    return Err(said(format!("{} was not sent: {}", failed.name, failed.reason)));
  }

  Ok(format!("voice: sent {bytes} bytes to chat {chat_id}"))
}

// The words spoken into a file of their own. Nothing is synthesised from words the reading gate has not returned, and
// a gate that cannot answer means nothing is said.
async fn spoken(context: &Context, asked: &Asked, words: &str, chat_id: i64) -> Result<PathBuf, String> {
  let gate = &context.units.gate;
  let names = voices::said_names(&context.db).await.map_err(said)?;
  let read = reading::read_for_voice(
    async |request: GateRequest| gate.run(reading::STAGE, &request).await,
    words,
    Some(chat_id),
    &names,
    None,
  )
  .await;
  let Some(reading) = read.spoken else {
    let fault = read.fault.unwrap_or(crate::script::Fault::NoAnswer);

    return Err(said(format!("the reading gate did not answer for the line: {fault}")));
  };
  let out = std::env::temp_dir().join(format!("bmo-voice-{}.ogg", now_ms()));
  let voice = chosen(context, asked).await?;

  if !voices::node_names(&context.db).await.map_err(said)?.is_empty() {
    let speech = Speech {
      db: &context.db,
      units: &context.units,
      settings: &context.settings,
      voice: &voice,
      text: &reading,
      out: &out,
      cap_ms: POLICY.run_cap_ms,
      wait_ms: QUEUE_WAIT_MS,
      clock: None,
      node_wait_ms: None,
    };
    let heard = node::speak_through_free_node(speech, async || true).await.map_err(said)?;

    eprintln!(
      "voice: {} spoke {:.1}s of {} in {:.1}s (weights {:.1}s)",
      heard.node,
      heard.spoken.seconds,
      heard.spoken.checkpoint,
      heard.spoken.speak_seconds,
      heard.spoken.load_seconds
    );

    return Ok(out);
  }

  // The hand tool stands in the same line as a briefing does: there is one card, and a person at a shell has no more
  // claim on it than a room does.
  match synth::speak(&Words::Line(&reading), &out, &voice, &context.settings).await {
    Ok(()) => Ok(out),
    Err(Unspoken::NoTurn) => Err(said(
      "a briefing is already on the synthesiser and the wait ran out; try again when it is done.",
    )),
    Err(Unspoken::Failed(why)) => {
      let _ = std::fs::remove_file(Path::new(&out));

      Err(said(format!("the synthesiser did not speak the line: {why}")))
    }
  }
}

// The voice this run speaks in, refused out loud rather than silently swapped: a name the table does not hold is a
// typo, and answering a typo with some other voice is how two clips that are the same clip get compared.
async fn chosen(context: &Context, asked: &Asked) -> Result<VoiceRow, String> {
  let named = match &asked.voice {
    Some(named) => named.clone(),
    None => voice_of(context, &context.settings.agent).await.map_err(said)?.unwrap_or_default(),
  };

  if let Some(row) = voices::voice(&context.db, &named).await.map_err(said)? {
    return Ok(row);
  }

  let held = voices::voice_names(&context.db).await.map_err(said)?;

  Err(said(format!(
    "there is no voice called {named}. The table holds: {}",
    if held.is_empty() { "nothing yet".to_owned() } else { held.join(", ") }
  )))
}
