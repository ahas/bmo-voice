//! A briefing: a line said out loud in a room in an agent's trained voice (`docs/voice-briefing.md`). Launching one
//! opens its run in agency's ledger and has agency start the runner apart; `voice say` speaks one while its caller
//! waits.

pub mod hand;
pub mod runner;

use std::collections::BTreeMap;
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};

use bmo_markup::js;

use crate::context::Context;
use crate::now_ms;
use crate::policy::POLICY;
use crate::store::briefings::{self, FULL, Opening, SUMMARY};
use crate::store::voices;
use crate::units::agency;

/// The agent a briefing's run stands under in agency's ledger.
pub const AGENT: &str = "voice";

/// The words the unit command carries a briefing's run by.
pub const RUNNER: &str = "voice-runner";

// How long a summary's title may run: it stands on one line of the listeners and of a notification.
const TITLE_MOST: usize = 60;

/// Why a launch was turned away before anything was opened: its code and its English sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
  pub code: &'static str,
  pub error: String,
}

impl Refusal {
  fn new(code: &'static str, error: impl Into<String>) -> Self {
    Self {
      code,
      error: error.into(),
    }
  }
}

impl std::fmt::Display for Refusal {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(&self.error)
  }
}

/// One briefing as it is launched: the room, the words, whose voice, who set it off, the line it voices or
/// belongs to, whether it is a summary with its title, whether it is a line's full reading kept on the line or a
/// document's narration kept as its sound, the emotion it is said with, and the road it goes out on where it is not
/// the room's own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
  pub chat_id: i64,
  pub text: String,
  pub agent: Option<String>,
  pub started_by: String,
  pub voices: Option<i64>,
  pub summary: bool,
  pub title: Option<String>,
  pub keep: bool,
  pub document: Option<i64>,
  pub emotion: Option<String>,
  pub via: Option<String>,
}

/// A briefing launched: its run in the ledger and the unit it runs as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launched {
  pub run_id: i64,
  pub unit: String,
}

/// What the launcher tells whoever called it: the whole answer, so a caller lets go of it and does not say the
/// briefing in text as well.
pub fn launched(unit: &str, chat_id: i64) -> String {
  format!(
    "voice briefing launched as {unit}; it speaks to chat {chat_id} by itself when it is done. Do not wait for it and do not say the briefing in text as well."
  )
}

/// Why a launch is turned away before anything is asked of another unit, or none for one that may go ahead: it
/// names who set it off, has something to say, says it with an emotion of the set, and a summary's title is one
/// bounded line.
pub fn refusal(launch: &Launch) -> Option<Refusal> {
  if launch.started_by.trim().is_empty() {
    return Some(Refusal::new(
      "nobody_named",
      "a voice briefing has to say who launched it: --by chat:<chatId> from a session, or the launcher's own name.",
    ));
  }

  if js::trim(&launch.text).is_empty() {
    return Some(Refusal::new(
      "nothing_to_say",
      "a voice briefing has to have something to say: the words are read from standard input, or given with --text.",
    ));
  }

  if let Some(emotion) = launch.emotion.as_ref().filter(|emotion| !POLICY.emotions.contains(emotion)) {
    return Some(Refusal::new(
      "unknown_emotion",
      format!(
        "a voice briefing's emotion is one of {}; \"{emotion}\" is not.",
        POLICY.emotions.join(", ")
      ),
    ));
  }

  let title = launch.title.as_deref().map(str::trim);

  if title
    .is_some_and(|title| title.is_empty() || title.contains('\n') || title.chars().count() > TITLE_MOST)
  {
    return Some(Refusal::new(
      "bad_launch",
      format!(
        "a summary's title is one line of at most {TITLE_MOST} characters saying what the briefing is about."
      ),
    ));
  }

  None
}

/// Why every briefing is refused now: the voice briefing's switch stands off. None where it is on; a switch that
/// cannot be read is an error.
pub async fn switched_off(context: &Context) -> Result<Option<Refusal>, String> {
  let switch = context.units.receiver.switch().await.map_err(|why| why.to_string())?;

  Ok((!switch.on).then(|| {
    Refusal::new(
      "switched_off",
      "the voice briefing is switched off; nothing is spoken until it is switched on again. Say this in text only and do not try again.",
    )
  }))
}

/// The voice an agent's lines are spoken in, by its name in the voices table: the one its registration names, and
/// for the agent the settings name, where its registration is not written, the one marked current.
pub async fn voice_of(context: &Context, agent: &str) -> Result<Option<String>, String> {
  match context.units.agency.agent(agent).await.map_err(|why| why.to_string())? {
    Some(registered) => Ok(registered.voice()),
    None if agent == context.settings.agent => {
      voices::current_voice(&context.db).await.map_err(|why| why.to_string())
    }
    None => Ok(None),
  }
}

/// Why an agent's line is not spoken, or none where its registration names a voice: an agent with none is never
/// spoken, whatever the switch says.
pub async fn voiceless(context: &Context, agent: &str) -> Result<Option<Refusal>, String> {
  if voice_of(context, agent).await?.is_some() {
    return Ok(None);
  }

  Ok(Some(Refusal::new(
    "voiceless",
    format!(
      "the agent {agent} has no voice, so nothing is spoken. Say this in text only and do not try again."
    ),
  )))
}

// The runner's arguments beyond the four it always takes, in the order the runner reads them. A summary's title is
// read off its row, never an argument.
fn runner_flags(launch: &Launch) -> Vec<String> {
  let mut flags = vec![];
  let named = [
    ("--emotion", launch.emotion.clone()),
    ("--via", launch.via.clone()),
    ("--voices", launch.voices.map(|voices| voices.to_string())),
  ];

  for (flag, value) in named {
    if let Some(value) = value {
      flags.extend([flag.to_owned(), value]);
    }
  }

  if launch.summary {
    flags.push("--summary".to_owned());
  }

  if launch.keep {
    flags.push("--keep".to_owned());
  }

  if let Some(document) = launch.document {
    flags.extend(["--document".to_owned(), document.to_string()]);
  }

  if let Some(agent) = &launch.agent {
    flags.extend(["--agent".to_owned(), agent.clone()]);
  }

  flags
}

// The unit command a briefing's run is: the `bmo-voice` binary beside the running one, told its words' file, its run
// and its unit, and the settings the running process reads.
fn runner_program(
  launch: &Launch,
  text_path: &Path,
  run_id: i64,
  unit: &str,
) -> Result<agency::Program, String> {
  let binary = std::env::current_exe().map_err(|why| format!("the running binary: {why}"))?;
  let folder = binary.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
  let program = folder.join("bmo-voice");
  let args = [RUNNER.to_owned(), launch.chat_id.to_string()]
    .into_iter()
    .chain([text_path.to_string_lossy().into_owned(), run_id.to_string(), unit.to_owned()])
    .chain(runner_flags(launch))
    .collect();
  let mut environment = BTreeMap::new();

  if let Ok(settings) = crate::settings::Settings::path() {
    environment
      .insert(crate::settings::SETTINGS_VARIABLE.to_owned(), settings.to_string_lossy().into_owned());
  }

  Ok(agency::Program {
    program: program.to_string_lossy().into_owned(),
    args,
    cwd: folder.to_string_lossy().into_owned(),
    environment,
  })
}

/// A briefing launched: refused in order before anything exists that the next would have to take back — what the
/// launch says, the switch, the agent's voice — then its words written where only the runner reads them, its run
/// opened in agency's ledger, its row written, and its runner started in its agent's host. The launched run, or
/// the refusal saying why nothing was started; an error where a unit it needs did not answer.
pub async fn launch(context: &Context, launch: &Launch) -> Result<Result<Launched, Refusal>, String> {
  if let Some(refused) = refusal(launch) {
    return Ok(Err(refused));
  }

  if let Some(refused) = switched_off(context).await? {
    return Ok(Err(refused));
  }

  let speaker = launch.agent.clone().unwrap_or_else(|| context.settings.agent.clone());

  if let Some(refused) = voiceless(context, &speaker).await? {
    return Ok(Err(refused));
  }

  let now = now_ms();
  let unit = format!("bmo-voice-{now}");
  let text_path = std::env::temp_dir().join(format!("{unit}.txt"));

  std::fs::OpenOptions::new()
    .write(true)
    .create(true)
    .truncate(true)
    .mode(0o600)
    .open(&text_path)
    .and_then(|mut file| file.write_all(js::trim(&launch.text).as_bytes()))
    .map_err(|why| format!("the briefing's words could not be written: {why}"))?;

  let run_id = context
    .units
    .agency
    .open_run(&agency::Opening {
      agent: AGENT.to_owned(),
      chat_id: Some(launch.chat_id),
      unit: unit.clone(),
      depth: 0,
      cap_ms: Some(POLICY.run_cap_ms),
      started_at: now / 1000,
      started_by: Some(launch.started_by.clone()),
    })
    .await
    .map_err(|why| why.to_string())?;

  let kind = if launch.summary {
    Some(SUMMARY)
  } else if launch.voices.is_some() {
    Some(FULL)
  } else {
    None
  };

  if let Some(kind) = kind {
    briefings::open(
      &context.db,
      &Opening {
        run_id,
        chat_id: launch.chat_id,
        message_id: launch.voices,
        kind,
        title: launch.title.as_deref().map(str::trim),
        awaits_line: launch.summary && launch.voices.is_none(),
        started_at: now,
      },
    )
    .await
    .map_err(|why| why.to_string())?;
  }

  let room = context
    .units
    .receiver
    .room_title(launch.chat_id)
    .await
    .unwrap_or_else(|_| launch.chat_id.to_string());
  let title = format!("voice briefing in {room}");
  let program = runner_program(launch, &text_path, run_id, &unit)?;

  match context.units.agency.launch(run_id, &title, AGENT, &program).await {
    Ok(()) => Ok(Ok(Launched { run_id, unit })),
    Err(why) => {
      let _ = std::fs::remove_file(&text_path);
      let _ = briefings::finished(&context.db, run_id, now_ms()).await;

      Ok(Err(Refusal::new(
        "not_up",
        format!("the voice briefing's run {unit} did not come up; nothing was said: {why}"),
      )))
    }
  }
}
