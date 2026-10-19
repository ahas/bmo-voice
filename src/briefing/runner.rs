//! `runner voice`, the half of a briefing that speaks (`docs/voice-briefing.md`, 「The briefing」): it runs apart in
//! its agent's host, so nothing waits on it. It reads the line through the reading gate — a full reading polished
//! for the ear first — has it spoken in the voice the agent's registration names on a free voice node or this
//! machine's card, held to the run's clock past its cap, puts the voice message in the room through the receiver,
//! files the briefing for the listeners and closes its run.
//!
//! A full reading is kept on its line and a narration as its document's sound, and neither goes to a road. A
//! summary goes out only after the earlier ones of its room, and one a newer summary replaced says nothing. Every
//! ending is written once: a briefing switched off or of an agent with no voice closes as refused, a replaced
//! summary as superseded, and a failure tells the room, once, that no voice is coming. The words, the reading made
//! of them and the sound live only as long as the run and are taken off the disk however it ends.

use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::{AGENT, voice_of, voiceless};
use crate::context::Context;
use crate::now_ms;
use crate::policy::POLICY;
use crate::script::reading::{self, Reading};
use crate::script::{GateRequest, polish, spoken};
use crate::speech::clock;
use crate::speech::node::{self, NotSpoken, RunClock, Speech};
use crate::speech::synth::{self, Fault, QUEUE_WAIT_MS, Unspoken, Words};
use crate::store::briefings;
use crate::store::voices::{self, VoiceRow};
use crate::units::agency::Note;
use crate::units::receiver::Notice;

// How often a run standing in a line looks again for its turn.
const TURN_POLL_MS: u64 = 2_000;

// The longest one keep of a run's clock lasts, which a node's own asker is given past the cap since it cannot hear
// a keep once it is running.
const LONGEST_KEEP_MS: i64 = 120 * 60_000;

/// What the runner is started with: the room, the file holding the words, the run its launcher opened and the unit
/// it runs as, the message the voice answers and the line it voices, whether the reading goes under the voice, the
/// emotion the listeners are told, whether it is a summary, a full reading kept on its line or a document's
/// narration, and the agent whose voice speaks it.
#[derive(Clone, Debug, Default)]
pub struct Started {
  pub chat_id: i64,
  pub text_path: PathBuf,
  pub run_id: Option<i64>,
  pub unit: String,
  pub reply_to: Option<i64>,
  pub voices: Option<i64>,
  pub transcript: bool,
  pub emotion: Option<String>,
  pub summary: bool,
  pub keep: bool,
  pub document: Option<i64>,
  pub agent: Option<String>,
}

/// A briefing as its listeners are told it went out (`docs/boundary-api.md`, 「The listener stream」).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Frame {
  pub chat: i64,
  pub message: i64,
  pub text: String,
  pub title: Option<String>,
  pub seconds: Option<f64>,
  pub sent_at: i64,
  pub emotion: Option<String>,
  pub face: Option<String>,
  pub autoplay: bool,
}

// How a run ends short of speaking: refused by the switch or the agent's voice, replaced by a newer summary, or
// failed, with the fault the room is told where it is told one.
enum Stop {
  Refused(&'static str),
  Superseded,
  Failed { reason: String, fault: Option<Fault> },
}

fn failed(reason: impl Into<String>, fault: Fault) -> Stop {
  Stop::Failed {
    reason: reason.into(),
    fault: Some(fault),
  }
}

fn unanswered(reason: impl std::fmt::Display) -> Stop {
  Stop::Failed {
    reason: reason.to_string(),
    fault: None,
  }
}

/// How a run ended: the line it says where it ended well, else the exit code and what it says on its error stream.
pub type Ended = Result<String, (i32, String)>;

/// One briefing spoken, from its words to its run's ending.
pub async fn voice_runner(context: &Context, started: Started) -> Ended {
  if started.text_path.as_os_str().is_empty() {
    return Err((2, "usage: bmo-voice voice-runner <chatId> <textFile> [runId] [unit]".to_owned()));
  }

  let mut run = Run::new(context, started);
  let ended = tokio::select! {
    ended = run.go() => ended,
    signal = stopped() => Err(failed(format!("stopped by {signal}"), Fault::Stopped)),
  };

  match ended {
    Ok(said) => Ok(said),
    Err(stop) => run.end(stop).await,
  }
}

// The first signal that asks the run to stop, by its name.
async fn stopped() -> &'static str {
  use tokio::signal::unix::{SignalKind, signal};

  let (Ok(mut term), Ok(mut interrupt)) =
    (signal(SignalKind::terminate()), signal(SignalKind::interrupt()))
  else {
    return std::future::pending().await;
  };

  tokio::select! {
    _ = term.recv() => "SIGTERM",
    _ = interrupt.recv() => "SIGINT",
  }
}

struct Run<'a> {
  context: &'a Context,
  started: Started,
  agent: String,
  keep: bool,
  started_at: i64,
  reading_path: PathBuf,
  out_path: PathBuf,
  told: bool,
  closed: bool,
}

impl<'a> Run<'a> {
  fn new(context: &'a Context, started: Started) -> Self {
    let stem = if started.unit.is_empty() {
      format!("bmo-voice-{}", now_ms())
    } else {
      started.unit.clone()
    };

    Self {
      agent: started.agent.clone().unwrap_or_else(|| context.settings.agent.clone()),
      keep: started.keep && started.voices.is_some(),
      started_at: now_ms(),
      reading_path: std::env::temp_dir().join(format!("{stem}.reading.txt")),
      out_path: std::env::temp_dir().join(format!("{stem}.ogg")),
      told: false,
      closed: false,
      context,
      started,
    }
  }

  fn narration(&self) -> bool {
    self.started.document.is_some()
  }

  // The name the run's clock and log stand under.
  fn run_name(&self) -> String {
    if self.started.unit.is_empty() {
      format!("voice:{}", std::process::id())
    } else {
      self.started.unit.clone()
    }
  }

  async fn note(&self, text: String) {
    if self.started.unit.is_empty() {
      eprintln!("voice: {text}");
      return;
    }

    let note = Note {
      agent: AGENT.to_owned(),
      run: self.started.unit.clone(),
      chat: Some(self.started.chat_id),
      text,
      state: None,
    };

    if let Err(why) = self.context.units.agency.note(&note).await {
      eprintln!("voice: a note could not be written into the run's log: {why}");
    }
  }

  // Everything the run was given or made taken off the disk; what is already gone is nothing to clear.
  fn clear_files(&self) {
    for path in [&self.started.text_path, &self.reading_path, &self.out_path] {
      let _ = std::fs::remove_file(path);
    }
  }

  // How the run ended, written once: the first ending reached is the real one. The line the voice belongs to is
  // told again however the run ended, so a screen standing the briefing on it reads it afresh.
  async fn close(&mut self, outcome: &str) {
    let Some(run_id) = self.started.run_id.filter(|_| !self.closed) else { return };
    let now = now_ms();

    self.closed = true;

    if let Err(why) = self.context.units.agency.close_run(run_id, now / 1000, outcome).await {
      eprintln!("voice: the run could not be closed: {why}");
    }

    if let Err(why) = briefings::finished(&self.context.db, run_id, now).await {
      eprintln!("voice: the briefing's end could not be written: {why}");
    }

    if let Some(voices) = self.started.voices {
      let _ = self.context.units.receiver.message_changed(self.started.chat_id, voices).await;
    }
  }

  // Whoever was going to hear the briefing hears instead that it is not coming, from the room's own voice, once. A
  // full reading's chip shows its failure itself and a narration belongs to a document that stands whole without
  // it, so the room is told nothing of either.
  async fn tell_room(&mut self, notice: Notice) {
    self.told = true;

    if self.keep || self.narration() {
      return;
    }

    let notice = Notice {
      kind: Some(POLICY.failed_kind.clone()),
      ttl_ms: None,
      ..notice
    };

    if let Err(why) = self.context.units.receiver.notice(self.started.chat_id, &notice, now_ms()).await {
      eprintln!("voice: the failure could not be told to the room: {why}");
    }
  }

  // A run that ends short of speaking, written down, its files cleared and its run closed.
  async fn end(&mut self, stop: Stop) -> Ended {
    let (outcome, said) = match stop {
      Stop::Refused(why) => {
        self.note(format!("refused: {why}")).await;
        ("refused", None)
      }
      Stop::Superseded => {
        eprintln!("voice: a newer summary of this room replaces this one");
        self.note("superseded: a newer summary of the room replaces it".to_owned()).await;
        ("superseded", None)
      }
      Stop::Failed { reason, fault } => {
        self.note(format!("failed: {reason}")).await;

        // A summary that never got its turn tells nobody: its line already stands in the room as text, and a card
        // other work kept busy is nothing the room can act on.
        if let Some(fault) =
          fault.filter(|fault| !self.told && !(self.started.summary && *fault == Fault::Queue))
        {
          self.tell_room(synth::failure_notice(fault)).await;
        }

        ("failed", Some(format!("voice run failed: {reason}")))
      }
    };

    self.clear_files();
    self.close(outcome).await;

    match said {
      Some(said) => Err((1, said)),
      None => Ok(String::new()),
    }
  }

  // A briefing reached after the switch went off is not spoken: whoever launched it was already told it would come,
  // and the switch is the owner's.
  async fn stop_if_switched_off(&self) -> Result<(), Stop> {
    match super::switched_off(self.context).await {
      Ok(Some(refusal)) => {
        eprintln!("voice: {refusal}");
        Err(Stop::Refused("the voice briefing is switched off"))
      }
      Ok(None) => Ok(()),
      Err(why) => Err(unanswered(why)),
    }
  }

  async fn stop_if_superseded(&self) -> Result<(), Stop> {
    if superseded(self.context, &self.started).await {
      Err(Stop::Superseded)
    } else {
      Ok(())
    }
  }

  async fn go(&mut self) -> Result<String, Stop> {
    let chat_id = self.started.chat_id;

    self.stop_if_switched_off().await?;

    // An agent whose registration names no voice is never spoken: a run reached after its voice was taken away
    // closes as refused, as one reached after the switch went off does.
    if let Some(refusal) = voiceless(self.context, &self.agent).await.map_err(unanswered)? {
      eprintln!("voice: {refusal}");

      return Err(Stop::Refused("the agent has no voice"));
    }

    self.stop_if_superseded().await?;

    let written = std::fs::read_to_string(&self.started.text_path)
      .map_err(|why| unanswered(format!("the words could not be read: {why}")))?
      .trim()
      .to_owned();
    // Cleaned once, before anything is made of it: the sound is made from these words, so the same words are read,
    // spoken, captioned and written down.
    let said = clean(self.context, &written, Some(chat_id)).await;

    self.note(format!("{} characters · chat {chat_id}", said.chars().count())).await;

    let spoken = self.read(&said).await?;
    let voice = self.chosen().await?;
    let on_nodes = !voices::node_names(&self.context.db).await.map_err(unanswered)?.is_empty();

    self.wait_behind_summaries().await?;

    if on_nodes {
      self.speak_on_node(&voice, &spoken).await?;
    } else {
      self.speak_here(&voice).await?;
    }

    let audio = std::fs::read(&self.out_path)
      .map_err(|why| failed(format!("the spoken file could not be read: {why}"), Fault::Sound))?;

    if self.keep {
      return self.keep_sound(&audio, "reading").await;
    }

    if self.narration() {
      return self.keep_sound(&audio, "narration").await;
    }

    self.wait_for_earlier_summaries().await?;
    self.send(&said, &spoken, &audio).await?;
    self.clear_files();
    self.note(format!("spoke {} bytes to chat {chat_id}", audio.len())).await;
    self.close("done").await;

    Ok(format!("voice: spoke {} bytes to chat {chat_id}", audio.len()))
  }

  // Nothing reaches the synthesiser that the reading gate has not returned. A full reading is said again for the ear
  // first, unless its gate is switched off; a summary was written to be heard and never is. A line the gate could not
  // answer for is told to the room in the gate's own words, and nothing is spoken.
  async fn read(&mut self, said: &str) -> Result<String, Stop> {
    let chat_id = Some(self.started.chat_id);
    let gate = &self.context.units.gate;
    let names = voices::said_names(&self.context.db).await.map_err(unanswered)?;
    let polishing = self.keep && gate.switched_on(polish::STAGE).await.unwrap_or(true);
    let made = spoken::spoken_script(said);
    let polished = if polishing {
      let started = now_ms();
      let polish = polish::polish_for_voice(
        async |request: GateRequest| gate.run(polish::STAGE, &request).await,
        &made,
        chat_id,
      )
      .await;
      let seconds = format!("{:.1}", (now_ms() - started) as f64 / 1000.0);
      let note = match &polish.fault {
        None => format!("polished in {seconds}s: {}", polish.script),
        Some(fault) => format!("polishing failed in {seconds}s, the mechanical script is read: {fault}"),
      };

      self.note(note).await;

      Some(polish)
    } else {
      None
    };
    let read: Reading = reading::read_for_voice(
      async |request: GateRequest| gate.run(reading::STAGE, &request).await,
      said,
      chat_id,
      &names,
      polished,
    )
    .await;

    let Some(spoken) = read.spoken else {
      let fault = read.fault.unwrap_or(crate::script::Fault::NoAnswer);

      self.note(format!("the reading gate failed: {fault}")).await;
      // The gate's own notice says what the general line cannot — which words were refused and why — and it is
      // said instead of it, never beside it.
      self.tell_room(reading::voice_reading_notice(&fault)).await;

      return Err(unanswered(format!("the reading gate did not answer for the line: {fault}")));
    };

    let note = if read.changed {
      format!("read as: {spoken}")
    } else {
      "read as written: nothing to transliterate".to_owned()
    };

    self.note(note).await;
    std::fs::OpenOptions::new()
      .write(true)
      .create(true)
      .truncate(true)
      .mode(0o600)
      .open(&self.reading_path)
      .and_then(|mut file| std::io::Write::write_all(&mut file, spoken.as_bytes()))
      .map_err(|why| failed(format!("the reading could not be written: {why}"), Fault::Synth))?;

    Ok(spoken)
  }

  // The voice the agent's registration names. There is no other synthesiser to fall back on, so a name the voices
  // table does not hold says nothing and tells the room why.
  async fn chosen(&self) -> Result<VoiceRow, Stop> {
    let name = voice_of(self.context, &self.agent).await.map_err(unanswered)?;
    let row = match &name {
      Some(name) => voices::voice(&self.context.db, name).await.map_err(unanswered)?,
      None => None,
    };

    row.ok_or_else(|| {
      failed(
        format!("the voice {} is not in the voices table", name.as_deref().unwrap_or("(none)")),
        Fault::Voice,
      )
    })
  }

  // A full reading or a narration waits while any summary is being made: the live briefing is the one somebody is
  // waiting to hear now. The wait is held to what any run stands in a line.
  async fn wait_behind_summaries(&mut self) -> Result<(), Stop> {
    let db = &self.context.db;

    if !(self.keep || self.narration()) || !briefings::summary_being_made(db).await.unwrap_or(false) {
      return Ok(());
    }

    self.note("waiting behind the summaries being made".to_owned()).await;

    let since = now_ms();

    while briefings::summary_being_made(db).await.unwrap_or(false) && now_ms() - since < QUEUE_WAIT_MS {
      self.stop_if_switched_off().await?;
      tokio::time::sleep(Duration::from_millis(TURN_POLL_MS)).await;
    }

    Ok(())
  }

  // The voice spoken on a free node of the list and encoded here. Nodes that do not answer are told to the room as
  // their own fault, never quietly replaced by this machine's card. The node's own asker cannot hear a keep, so it is
  // given the cap and the longest one keep.
  async fn speak_on_node(&mut self, voice: &VoiceRow, spoken: &str) -> Result<(), Stop> {
    self.note(format!("{}: spoken on a voice node", voice.name)).await;

    let cap = POLICY.run_cap_ms;
    let run = self.run_name();
    let held = RunClock {
      agency: &self.context.units.agency,
      run: &run,
    };
    let speech = Speech {
      db: &self.context.db,
      units: &self.context.units,
      settings: &self.context.settings,
      voice,
      text: spoken,
      out: &self.out_path,
      cap_ms: cap,
      wait_ms: QUEUE_WAIT_MS,
      clock: Some(&held),
      node_wait_ms: Some(cap + LONGEST_KEEP_MS),
    };
    let (context, started) = (self.context, &self.started);
    let heard = node::speak_through_free_node(speech, async || still_wanted(context, started).await).await;

    match heard {
      Ok(heard) => {
        let mut how = vec![];

        if heard.waited_ms >= 1_000 {
          how.push(format!("waited {:.0}s for a free node", heard.waited_ms as f64 / 1000.0));
        }

        if heard.card_held {
          how.push("stood in the line of a card other work held".to_owned());
        }

        let how = if how.is_empty() { String::new() } else { format!(" ({})", how.join(", ")) };

        self
          .note(format!(
            "node {}{how}: {:.1}s of speech · spoken in {:.1}s · weights {:.1}s · {}",
            heard.node,
            heard.spoken.seconds,
            heard.spoken.speak_seconds,
            heard.spoken.load_seconds,
            heard.spoken.checkpoint
          ))
          .await;

        Ok(())
      }
      Err(error) => {
        if matches!(error, NotSpoken::Withdrawn) {
          self.stop_if_switched_off().await?;
          self.stop_if_superseded().await?;
        }

        let fault = match &error {
          NotSpoken::Node(error) if error.kind == node::Failure::Taken => Fault::Queue,
          NotSpoken::Node(_) => Fault::Node,
          NotSpoken::Withdrawn | NotSpoken::Here(_) => Fault::Synth,
        };

        Err(failed(format!("the voice node did not speak the line: {error}"), fault))
      }
    }
  }

  // The voice spoken on this machine's own card, where no node is listed: the card is one and the model is loaded
  // onto it for every line, so the call stands in a line, and the switch and the summaries are asked again when its
  // turn comes.
  async fn speak_here(&mut self, voice: &VoiceRow) -> Result<(), Stop> {
    // Weights this machine has not been given are a voice that is not installed here.
    if !Path::new(&voice.checkpoint).join("lora_weights.safetensors").exists() {
      return Err(failed(
        format!("{}'s weights are not on this machine ({})", voice.name, voice.checkpoint),
        Fault::Voice,
      ));
    }

    let settings = &self.context.settings;
    let lock = synth::lock_path(settings);

    if let Err(why) = synth::ready_lock(&lock) {
      eprintln!("voice: the card's lock file could not be made: {why}");
    }

    if synth::queue_busy(&lock) {
      self.note("another briefing is speaking, so this one stands in line".to_owned()).await;

      let since = now_ms();

      while synth::queue_busy(&lock) && now_ms() - since < QUEUE_WAIT_MS {
        self.stop_if_switched_off().await?;
        self.stop_if_superseded().await?;
        tokio::time::sleep(Duration::from_millis(TURN_POLL_MS)).await;
      }
    }

    self.stop_if_switched_off().await?;
    self.stop_if_superseded().await?;

    let cap = POLICY.run_cap_ms;
    let run = self.run_name();
    let words = Words::File(&self.reading_path);
    // A call cut short ends the entry's process group, the synthesiser with it.
    let spoken = synth::speak(&words, &self.out_path, voice, settings);
    // The cap covers the wait as well as the speaking: standing in a line is time the room is waiting just as much.
    let watch = clock::watch(&self.context.units.agency, &run, now_ms(), cap);
    let ended = tokio::select! {
      ended = spoken => ended,
      () = watch => Err(Unspoken::Failed("the run passed its cap and was not kept".to_owned())),
    };

    match ended {
      Ok(()) => Ok(()),
      // The turn never came: its own ending rather than a failure of the synthesiser, which was never reached.
      Err(Unspoken::NoTurn) => Err(failed("the wait for the synthesiser ran out", Fault::Queue)),
      Err(Unspoken::Failed(why)) => {
        Err(failed(format!("the synthesiser did not speak the line: {why}"), Fault::Synth))
      }
    }
  }

  // A full reading is kept as its line's own and a narration as its document's sound, each under the voice's folder
  // by its run, and goes nowhere: no road and no frame.
  async fn keep_sound(&mut self, audio: &[u8], what: &str) -> Result<String, Stop> {
    let Some(run_id) = self.started.run_id else {
      return Err(failed(format!("a {what} without a run has nowhere to be kept"), Fault::Sound));
    };
    let kept = kept_sound(&self.context.settings, run_id);
    let written = kept
      .parent()
      .map_or(Ok(()), std::fs::create_dir_all)
      .and_then(|()| std::fs::write(&kept, audio));

    written.map_err(|why| failed(format!("the {what} could not be kept: {why}"), Fault::Sound))?;

    if let Err(why) = briefings::kept(&self.context.db, run_id, &kept.to_string_lossy()).await {
      eprintln!("voice: the kept sound could not be written on its row: {why}");
    }

    let of = match (self.started.voices, self.started.document) {
      (Some(line), _) => format!("line {line}"),
      (None, Some(document)) => format!("document {document}"),
      (None, None) => "nothing".to_owned(),
    };

    self.clear_files();
    self.note(format!("kept {} bytes as the {what} of {of}", audio.len())).await;
    self.close("done").await;

    Ok(format!("voice: kept {} bytes as the {what} of {of}", audio.len()))
  }

  // A summary goes out only once the earlier ones of its room have been made and heard, with a short pause after the
  // last, so the listeners play them one after another. The wait is held to the run's cap like every other.
  async fn wait_for_earlier_summaries(&self) -> Result<(), Stop> {
    let Some(run_id) = self.started.run_id.filter(|_| self.started.summary) else {
      return Ok(());
    };

    loop {
      self.stop_if_superseded().await?;

      let earlier = briefings::earlier_summaries(&self.context.db, self.started.chat_id, run_id)
        .await
        .unwrap_or_default();
      let wait = synth::summary_wait_ms(earlier.making, earlier.heard_until, now_ms());

      if wait == 0 || now_ms() - self.started_at > POLICY.run_cap_ms {
        return Ok(());
      }

      tokio::time::sleep(Duration::from_millis(wait.min(TURN_POLL_MS as i64) as u64)).await;
    }
  }

  // The voice message put in the room through the receiver, the reading as its caption unless the line it answers
  // already stands above it. The row names the voice message before any listener is told it landed, and every
  // listener is told to play what just went out.
  async fn send(&mut self, said: &str, spoken: &str, _audio: &[u8]) -> Result<(), Stop> {
    let chat_id = self.started.chat_id;
    let receiver = &self.context.units.receiver;
    let caption = self.started.transcript.then_some(spoken);
    let sent = receiver
      .send_file(chat_id, &self.out_path, caption, self.started.reply_to)
      .await
      .map_err(|why| failed(why.to_string(), Fault::Sending))?;
    let sent_at = now_ms();
    let Some(message_id) = sent.message_id.or_else(|| sent.sent.first().map(|one| one.message_id)) else {
      let why = sent
        .failed
        .first()
        .map_or_else(|| "no message came back".to_owned(), |one| one.reason.clone());

      return Err(failed(format!("the voice message could not be sent: {why}"), Fault::Sending));
    };

    if let Some(run_id) = self.started.run_id
      && let Err(why) = briefings::sent(&self.context.db, run_id, message_id).await
    {
      eprintln!("voice: the voice message could not be written on its row: {why}");
    }

    let seconds = std::fs::read(&self.out_path).ok().and_then(|audio| synth::ogg_opus_seconds(&audio));
    let briefing = match self.started.run_id {
      Some(run_id) => briefings::of_run(&self.context.db, run_id).await.ok().flatten(),
      None => None,
    };

    if let (Some(run_id), Some(_)) = (self.started.run_id, &briefing) {
      let _ = briefings::played(&self.context.db, run_id, sent_at, seconds).await;
    }

    let emotion = self.started.emotion.clone().filter(|emotion| POLICY.emotions.contains(emotion));
    let face = face_of(self.context, emotion.as_deref()).await;
    let autoplay = receiver.switch().await.map(|switch| switch.autoplay).unwrap_or(true);
    let frame = Frame {
      chat: chat_id,
      message: message_id,
      text: said.to_owned(),
      title: briefing.and_then(|briefing| briefing.title),
      seconds,
      sent_at,
      emotion,
      face,
      autoplay,
    };

    // A frame that does not land costs the listeners this one briefing and nothing else.
    if let Err(why) = file_frame(self.context, &frame).await {
      eprintln!("voice: the listeners could not be told of the briefing: {why}");
    }

    Ok(())
  }
}

/// Where a run's kept sound stands under the voice's folder.
pub fn kept_sound(settings: &crate::settings::Settings, run_id: i64) -> PathBuf {
  settings.voice_folder().join("sounds").join(format!("{run_id}.ogg"))
}

// The briefing filed on the voice's own listener stream, through the service the settings name.
async fn file_frame(context: &Context, frame: &Frame) -> Result<(), String> {
  let settings = &context.settings;
  let token = settings.token(&settings.token_file).map_err(|why| why.to_string())?;
  let mut request = reqwest::Client::new()
    .post(format!("{}/briefings/heard", settings.address.trim_end_matches('/')))
    .json(frame);

  if let Some(token) = token {
    request = request.bearer_auth(token);
  }

  let answer = request.send().await.map_err(|why| why.to_string())?;

  if answer.status().is_success() {
    Ok(())
  } else {
    Err(format!("the voice service answered {}", answer.status()))
  }
}

// The face a briefing shows, as the face unit's `face/expression` entry answers for its emotion; none where the
// entry does not answer, which the log says and the listeners meet with the emotion alone.
async fn face_of(context: &Context, emotion: Option<&str>) -> Option<String> {
  let arguments = emotion.map_or_else(|| json!({}), |emotion| json!({ "emotion": emotion }));
  let answer = match context
    .units
    .pocket
    .call("face/expression", arguments, &context.settings.data_root)
    .await
  {
    Ok(answer) if answer["isError"] != true => answer,
    Ok(answer) => {
      eprintln!("voice: the face's entry refused the briefing's face: {}", answer["content"]);

      return None;
    }
    Err(why) => {
      eprintln!("voice: the face's entry did not answer for the briefing's face: {why}");

      return None;
    }
  };

  answer["structuredContent"]["expression"].as_str().map(str::to_owned)
}

// A summary a newer one of its room has replaced before it went out: the newer one was written knowing it.
async fn superseded(context: &Context, started: &Started) -> bool {
  match started.run_id.filter(|_| started.summary) {
    Some(run_id) => briefings::newer_summary(&context.db, started.chat_id, run_id).await.unwrap_or(false),
    None => false,
  }
}

// Whether a line waiting for a node is still to be said: the switch on and no newer summary in its place.
async fn still_wanted(context: &Context, started: &Started) -> bool {
  matches!(super::switched_off(context).await, Ok(None)) && !superseded(context, started).await
}

/// Words made safe to say out loud, as the gate's sanitiser reads them; where it cannot answer, the words go on as
/// they are and the log says so.
pub async fn clean(context: &Context, text: &str, chat_id: Option<i64>) -> String {
  let label = "the words a voice message the agent is about to send says out loud";

  match context.units.gate.sanitised(text, label, chat_id).await {
    Ok(safe) => safe,
    Err(why) => {
      eprintln!("voice: the sanitiser did not answer, so the words go on as they are: {why}");
      text.to_owned()
    }
  }
}
