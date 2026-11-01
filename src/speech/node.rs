//! A line spoken on a voice node (docs/voice-plan.md, 「The voice servers on the voice nodes」, 「Which node speaks
//! a line」, 「How a line crosses」, 「When the nodes are out of reach」). The server listens on the node's loopback
//! alone, so every call goes through the node's own shell: the request handed in on standing input, the asker
//! started apart on the node, its answer asked after until it is there, the wav carried back and every file of
//! the call removed. The OGG/Opus encode stays on this machine.
//!
//! Which node speaks a line is the card the arbiter's master grants it ([`super::card`]); how that node is reached is
//! the node service's answer for it. The card is held for the whole of the line, and a node out of reach is never
//! quietly replaced by this machine's card.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use super::{card, shell_word, synth};
use crate::now_ms;
use crate::settings::Settings;
use crate::store::voices::VoiceRow;
use crate::units::Units;
use crate::units::arbiter::Ask;
use crate::units::hub::{ExecEnd, HubTarget, hub_exec};
use crate::units::node::Reach;

/// The voice's own name: the service the arbiter's master grants its cards of, and the one a node's row may reserve
/// the node for.
pub const SERVICE: &str = "voice";

// Where everything lands on the node, relative to its home, the port the server listens on there, and the token
// file beside the node's other configuration.
const PLACE: &str = "bmo-voice";
const PORT: u16 = 7432;
const TOKEN: &str = ".config/bmo-voice/token";

// The start of a call's command exits with this where the server is not installed on the node.
const NOT_INSTALLED: i32 = 4;

// The engine environment's interpreter under the node's place (`docs/voice-plan.md`, 「The environments」).
const ENGINE_PYTHON: &str = "fish-speech/.venv/bin/python";

// The longest one wait on the node lasts: a cut the clock asks for is heard within it.
const ASK_WAIT_SECONDS: i64 = 10;

// How long a call is waited past its time without a clock, for the asker to write its end.
const GRACE_MS: i64 = 5_000;

// ssh reports its own failure to connect, log in or keep the channel as this; every other code is the command's.
const SSH_FAILED: i32 = 255;

// How much of a file one command carries back from the node. A hub node's hub ends a command whose output passes
// 8 MiB, and base64 writes four bytes for every three, so a minute and a half of wav is past it in one piece.
const SLICE_BYTES: u64 = 4 * 1024 * 1024;

/// How a call to a node failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
  /// No connection: the node down, its hub or agent gone, the server not installed or not listening.
  Unreachable,
  /// No answer within the call's wait.
  Timeout,
  /// The server said no: a wrong token, a voice it does not hold.
  Refused,
  /// The server took the line and failed to speak it, or the node failed to start asking.
  Failed,
  /// An answer that is not a wav.
  BadAnswer,
  /// The node's card stayed held by other work for the whole of the server's own wait for its turn.
  Taken,
  /// The voice's files stand outside the voices root, the only layout a node mirrors.
  Elsewhere,
}

impl Failure {
  fn word(self) -> &'static str {
    match self {
      Self::Unreachable => "unreachable",
      Self::Timeout => "timeout",
      Self::Refused => "refused",
      Self::Failed => "failed",
      Self::BadAnswer => "bad-answer",
      Self::Taken => "taken",
      Self::Elsewhere => "elsewhere",
    }
  }
}

/// A node's failure: its kind, the server's status and its own reason where it gave them — never the line, so a
/// run's log may carry it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeError {
  pub kind: Failure,
  pub status: Option<i64>,
  pub reason: Option<String>,
}

impl NodeError {
  fn new(kind: Failure, status: Option<i64>, reason: Option<String>) -> Self {
    Self {
      kind,
      status,
      reason,
    }
  }
}

impl fmt::Display for NodeError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "voice node {}", self.kind.word())?;

    if let Some(status) = self.status {
      write!(f, " ({status})")?;
    }

    if let Some(reason) = &self.reason {
      write!(f, ": {reason}")?;
    }

    Ok(())
  }
}

/// Why a line was not spoken on a node: the node's own failure, or a failure here: the master's refusal of its card
/// or this machine's own encode step.
#[derive(Debug)]
pub enum NotSpoken {
  Node(NodeError),
  Here(String),
}

impl fmt::Display for NotSpoken {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Node(error) => error.fmt(f),
      Self::Here(why) => f.write_str(why),
    }
  }
}

impl From<NodeError> for NotSpoken {
  fn from(error: NodeError) -> Self {
    Self::Node(error)
  }
}

fn unreachable(reason: impl Into<String>) -> NodeError {
  NodeError::new(Failure::Unreachable, None, Some(reason.into()))
}

/// What a command run in a node's shell came to.
struct ShellRun {
  code: Option<i32>,
  stdout: Vec<u8>,
  stderr: String,
}

/// A node's own shell: a command run there as bash runs it, with bytes on its standing input. An ssh node is
/// reached over a batch login, a hub node through its hub; a node whose login answers in PowerShell runs no bash
/// command and is out of reach.
pub enum NodeShell {
  Ssh { args: Vec<String>, host: String },
  Hub(HubTarget),
  PowerShell(String),
}

impl NodeShell {
  // A failure to reach the node at all, as the call's error; a command that ran and failed is a run.
  async fn run(&self, command: &str, input: Option<&[u8]>) -> Result<ShellRun, String> {
    match self {
      Self::Ssh { args, host } => {
        let mut child = tokio::process::Command::new("ssh")
          .args(args)
          .arg(format!("bash -c {}", shell_word(command)))
          .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
          .stdout(Stdio::piped())
          .stderr(Stdio::piped())
          .kill_on_drop(true)
          .spawn()
          .map_err(|why| format!("ssh could not be started: {why}"))?;

        if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
          stdin
            .write_all(bytes)
            .await
            .map_err(|why| format!("ssh to {host} took no input: {why}"))?;
        }

        let done = child.wait_with_output().await.map_err(|why| format!("ssh to {host} failed: {why}"))?;
        let stderr = String::from_utf8_lossy(&done.stderr).into_owned();

        if done.status.code() == Some(SSH_FAILED) {
          return Err(format!("ssh to {host} failed: {}", tail(stderr.trim(), 300)));
        }

        Ok(ShellRun {
          code: done.status.code(),
          stdout: done.stdout,
          stderr,
        })
      }
      Self::Hub(target) => {
        let mut env = BTreeMap::new();
        let mut text = format!("{{\n{command}\n}} < /dev/null");

        if let Some(bytes) = input {
          env.insert("BMO_NODE_INPUT".to_owned(), STANDARD.encode(bytes));
          text = format!("printf '%s' \"$BMO_NODE_INPUT\" | base64 -d | {{\n{command}\n}}");
        }

        let lines = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let started = std::sync::Arc::new(Mutex::new(None));
        let gathered = std::sync::Arc::clone(&lines);
        let end = hub_exec(target, &text, &env, started, move |line: &str| {
          gathered.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(line.to_owned());
        })
        .await;
        let stdout = lines.lock().unwrap_or_else(std::sync::PoisonError::into_inner).join("\n");

        match end {
          ExecEnd::Exit { code, stderr } => Ok(ShellRun {
            code: code.and_then(|code| i32::try_from(code).ok()),
            stdout: stdout.into_bytes(),
            stderr,
          }),
          ExecEnd::Error(why) => Err(why),
        }
      }
      Self::PowerShell(address) => {
        Err(format!("{address} answers in Windows PowerShell and runs no bash command"))
      }
    }
  }

  // A whole file on the node, carried back as its bytes a slice to a command, each slice inside what one command
  // may answer with.
  async fn get(&self, path: &str) -> Result<Vec<u8>, String> {
    let word = shell_word(path);
    let unread =
      |ran: &ShellRun| format!("{path} could not be read on the node: {}", tail(ran.stderr.trim(), 300));
    let sized = self.run(&format!("stat -c %s {word}"), None).await?;

    if sized.code != Some(0) {
      return Err(unread(&sized));
    }

    let size: u64 = String::from_utf8_lossy(&sized.stdout)
      .trim()
      .parse()
      .map_err(|_| format!("{path} told no size on the node"))?;
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or_default());

    while (bytes.len() as u64) < size {
      let from = bytes.len() as u64 + 1;
      let ran = self
        .run(&format!("tail -c +{from} {word} | head -c {SLICE_BYTES} | base64 -w0"), None)
        .await?;

      if ran.code != Some(0) {
        return Err(unread(&ran));
      }

      let slice = STANDARD
        .decode(String::from_utf8_lossy(&ran.stdout).split_whitespace().collect::<String>())
        .map_err(|why| format!("{path} came back unreadable: {why}"))?;

      if slice.is_empty() {
        return Err(format!("{path} ended short of its {size} bytes on the node"));
      }

      bytes.extend_from_slice(&slice);
    }

    Ok(bytes)
  }
}

fn tail(text: &str, most: usize) -> String {
  let count = text.chars().count();

  text.chars().skip(count.saturating_sub(most)).collect()
}

/// The shell of a node by its name, as the node service says it is reached. A name the service does not list, a
/// node that cannot be reached as it stands, and a node its role reserves for other services are all the node out
/// of reach.
pub async fn node_shell(units: &Units, name: String) -> Result<NodeShell, NodeError> {
  let target = units
    .node
    .target(name.clone())
    .await
    .map_err(|why| unreachable(format!("there is no reachable node called {name}: {why}")))?;

  if let Some(role) =
    target.role.as_deref().filter(|role| !role.split('+').any(|service| service == SERVICE))
  {
    return Err(unreachable(format!("{name} is reserved for {role} and carries no {SERVICE}")));
  }

  match target.reach {
    Reach::Hub {
      hub_url,
      token_file,
      agent,
    } => Ok(NodeShell::Hub(HubTarget {
      hub_url,
      agent,
      token_file,
    })),
    Reach::Ssh {
      host, port, shell, ..
    } if shell.as_deref() == Some("powershell") => Ok(NodeShell::PowerShell(format!("{host}:{port}"))),
    Reach::Ssh { host, .. } => {
      let args = units
        .node
        .login(name.clone())
        .await
        .map_err(|why| unreachable(why.to_string()))?
        .ok_or_else(|| unreachable(format!("{name} has no SSH route")))?;

      Ok(NodeShell::Ssh { args, host })
    }
  }
}

// A path as a name under the voices root, or none where it stands outside.
fn under(root: &Path, path: &str) -> Option<String> {
  Path::new(path)
    .strip_prefix(root)
    .ok()
    .map(|name| name.to_string_lossy().into_owned())
    .filter(|name| !name.is_empty())
}

/// What the node is asked for one line in one voice: the row's folder named under the voices root, which is all the
/// node's copy shares, with the seed, the delivery and the sentence switch the row speaks with
/// (`docs/voice-plan.md`, 「What it answers」).
pub fn request(voice: &VoiceRow, text: &str, root: &Path) -> Result<Value, NodeError> {
  let checkpoint = under(root, &voice.checkpoint).ok_or_else(|| {
    NodeError::new(
      Failure::Elsewhere,
      None,
      Some(format!("{} is not under {}", voice.name, root.display())),
    )
  })?;

  Ok(json!({
    "text": text, "checkpoint": checkpoint, "seed": voice.seed, "style": voice.style, "sentences": voice.by_sentence,
  }))
}

// The node's interpreter running the server file, as a line of the node's shell.
fn on_node() -> String {
  format!(r#""$HOME/{PLACE}/{ENGINE_PYTHON}" "$HOME/{PLACE}/voice-fish.py""#)
}

// Starts the asker apart from the call and says where the call's files stand. The asker holds the call's lock
// file from before this returns until it has written its end, so the moment it lets go is the moment its line is
// there.
fn start_command(id: &str, wait_seconds: i64) -> String {
  let asker = on_node();

  [
    format!(r#"d="$HOME/{PLACE}/spoken""#),
    r#"mkdir -p "$d" && chmod 700 "$d" || exit 3"#.to_owned(),
    r#"find "$d" -type f -mmin +60 ! -name "*.lock" -delete 2>/dev/null"#.to_owned(),
    r#"find "$d" -type f -name "*.lock" -mmin +60 -exec flock -n {} rm -f {} \; 2>/dev/null"#.to_owned(),
    format!(
      r#"[ -x "$HOME/{PLACE}/{ENGINE_PYTHON}" ] || {{ echo "the voice server is not installed on this node" >&2; exit {NOT_INSTALLED}; }}"#
    ),
    "umask 077".to_owned(),
    format!(r#"cat > "$d/{id}.in" || exit 3"#),
    format!(r#"exec 9> "$d/{id}.lock" && flock -n 9 || exit 3"#),
    format!(
      r#"setsid sh -c '{asker} --ask --port {PORT} --token-file "$HOME/{TOKEN}" --request "$1/$2.in" --wav "$1/$2.wav" --timeout "$3" > "$1/$2.out" 2> "$1/$2.err"; echo $? > "$1/$2.code"' sh "$d" {} {wait_seconds} < /dev/null > /dev/null 2>&1 &"#,
      shell_word(id)
    ),
    "exec 9>&-".to_owned(),
    r#"printf '%s\n' "$d""#.to_owned(),
  ]
  .join("\n")
}

// Waits on the node, `seconds` at most, for the asker to let go of the call's lock, then says whether it has
// ended and what it wrote: `waiting`, `gone` where it ended without writing its end, or `done <code>` followed by
// its answer and the tail of its error stream folded onto one line.
fn await_command(dir: &str, id: &str, seconds: i64) -> String {
  let at = format!("{}/{}", shell_word(dir), shell_word(id));

  [
    format!("[ -f {at}.code ] || flock -w {seconds} -s {at}.lock true 2>/dev/null"),
    format!("if flock -n -s {at}.lock true 2>/dev/null; then free=1; else free=0; fi"),
    format!(
      r#"if [ -f {at}.code ]; then echo "done $(cat {at}.code)"; cat {at}.out; echo; tail -c 1000 {at}.err | tr '\n' ' '; elif [ $free = 1 ]; then echo gone; else echo waiting; fi"#
    ),
  ]
  .join("\n")
}

/// What the asker on the node wrote once it had its answer.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Asked {
  status: Option<i64>,
  error: Option<String>,
  seconds: Option<f64>,
  load_seconds: Option<f64>,
  speak_seconds: Option<f64>,
  checkpoint: Option<String>,
}

fn failure_of(asked: &Asked) -> NodeError {
  let error = asked.error.clone();

  let Some(status) = asked.status else {
    let kind = match error.as_deref() {
      Some(error) if error.starts_with("timeout") => Failure::Timeout,
      Some(error) if error.starts_with("token") => Failure::Refused,
      _ => Failure::Unreachable,
    };

    return NodeError::new(kind, None, error);
  };

  // The server's 503 is the node's card held by other work past the line's wait for it: the line did not get its
  // turn, as when every node stays taken.
  let kind = match status {
    503 => Failure::Taken,
    500.. => Failure::Failed,
    _ => Failure::Refused,
  };

  NodeError::new(kind, Some(status), error)
}

async fn ask_after(
  shell: &NodeShell,
  dir: &str,
  id: &str,
  given_up_at: &AtomicI64,
  wait_seconds: i64,
) -> Result<Asked, NodeError> {
  loop {
    let left = given_up_at.load(Ordering::SeqCst).saturating_sub(now_ms());
    let seconds = wait_seconds.min((left + 999) / 1000).max(1);
    let polled = shell.run(&await_command(dir, id, seconds), None).await.map_err(unreachable)?;
    let said = String::from_utf8_lossy(&polled.stdout).into_owned();
    let lines: Vec<&str> = said.split('\n').collect();
    let first = lines.first().map(|line| line.trim()).unwrap_or_default();

    if first == "gone" {
      return Err(NodeError::new(
        Failure::Failed,
        None,
        Some("the asker on the node ended without writing its end".to_owned()),
      ));
    }

    if let Some(code) = first.strip_prefix("done ").filter(|code| code.chars().all(|c| c.is_ascii_digit()))
    {
      if let Some(answer) = lines[1..].iter().find(|line| line.trim().starts_with('{'))
        && let Ok(asked) = serde_json::from_str::<Asked>(answer)
      {
        return Ok(asked);
      }

      let rest: Vec<&str> =
        lines[1..].iter().copied().filter(|line| !line.trim().starts_with('{')).collect();
      let rest = tail(rest.join(" ").trim(), 300);

      return Err(NodeError::new(
        Failure::Failed,
        None,
        Some(format!(
          "the asker on the node ended {code} without an answer{}",
          if rest.is_empty() { String::new() } else { format!(": {rest}") }
        )),
      ));
    }

    if given_up_at.load(Ordering::SeqCst) <= now_ms() {
      return Err(NodeError::new(Failure::Timeout, None, None));
    }
  }
}

/// The node's own account of a line it spoke.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spoken {
  pub seconds: f64,
  pub load_seconds: f64,
  pub speak_seconds: f64,
  pub checkpoint: String,
}

/// How one line is asked of a node: within `timeout_ms`, given up a grace past it, and the node's own asker
/// waiting `node_wait_ms`.
pub struct Call {
  pub timeout_ms: i64,
  pub node_wait_ms: Option<i64>,
}

// One line spoken on the node, as the wav it answered.
async fn speak_on_node(
  shell: &NodeShell,
  request: &Value,
  call: Call,
) -> Result<(Vec<u8>, Spoken), NodeError> {
  let id = format!("{:x}-{:08x}", now_ms(), rand::random::<u32>());
  let wait_seconds = (call.node_wait_ms.unwrap_or(call.timeout_ms) + 999) / 1000;
  let given_up_at = AtomicI64::new(now_ms() + call.timeout_ms + GRACE_MS);
  let dir = Mutex::new(None::<String>);
  let work = async {
    let input = request.to_string();
    let begun = shell
      .run(&start_command(&id, wait_seconds.max(1)), Some(input.as_bytes()))
      .await
      .map_err(unreachable)?;

    if begun.code == Some(NOT_INSTALLED) {
      let said = begun.stderr.trim();

      return Err(unreachable(if said.is_empty() {
        "the voice server is not installed on this node"
      } else {
        said
      }));
    }

    let named = String::from_utf8_lossy(&begun.stdout)
      .trim()
      .lines()
      .last()
      .unwrap_or_default()
      .trim()
      .to_owned();

    if begun.code != Some(0) || !named.starts_with('/') {
      return Err(NodeError::new(
        Failure::Failed,
        None,
        Some(format!("the node did not start asking: {}", tail(begun.stderr.trim(), 300))),
      ));
    }

    *dir.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(named.clone());

    let asked = ask_after(shell, &named, &id, &given_up_at, ASK_WAIT_SECONDS).await?;

    if asked.status != Some(200) {
      return Err(failure_of(&asked));
    }

    let wav = shell.get(&format!("{named}/{id}.wav")).await.map_err(unreachable)?;

    if wav.len() < 44 || &wav[..4] != b"RIFF" {
      return Err(NodeError::new(
        Failure::BadAnswer,
        asked.status,
        Some("the answer is not a wav".to_owned()),
      ));
    }

    Ok((
      wav,
      Spoken {
        seconds: asked.seconds.unwrap_or_default(),
        load_seconds: asked.load_seconds.unwrap_or_default(),
        speak_seconds: asked.speak_seconds.unwrap_or_default(),
        checkpoint: asked.checkpoint.unwrap_or_default(),
      },
    ))
  };
  let spoken = work.await;
  let dir = dir.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);

  // The line and its sound leave the node with the call; a node gone meanwhile keeps them until the next call's
  // sweep.
  if let Some(dir) = dir {
    let _ = shell.run(&format!("rm -f {}/{}.*", shell_word(&dir), shell_word(&id)), None).await;
  }

  spoken
}

/// What a node's server says of itself: the checkpoint it holds loaded, whether it is speaking this moment,
/// whether its card has room for the model, and whether other work holds the card's lock this moment.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Health {
  pub loaded: Option<String>,
  pub busy: bool,
  pub room: Option<bool>,
  pub card_taken: Option<bool>,
}

/// The node's server asked what it holds, with the asker's own short wait. A node that cannot be asked, or a
/// server that does not answer, is out of reach.
pub async fn health(shell: &NodeShell) -> Result<Health, NodeError> {
  let ran = shell
    .run(
      &format!(r#"{} --ask --port {PORT} --token-file "$HOME/{TOKEN}" --timeout 5"#, on_node()),
      None,
    )
    .await
    .map_err(unreachable)?;
  let said = String::from_utf8_lossy(&ran.stdout).into_owned();
  let last = said.trim().lines().last().unwrap_or_default();

  #[derive(Deserialize)]
  struct Answer {
    status: Option<i64>,
    error: Option<String>,
    health: Option<Health>,
  }

  let answer: Answer = serde_json::from_str(last).map_err(|_| {
    let stderr = ran.stderr.trim();

    unreachable(tail(if stderr.is_empty() { "the server's asker said nothing" } else { stderr }, 300))
  })?;

  match answer {
    Answer {
      status: Some(200),
      health: Some(health),
      ..
    } => Ok(health),
    Answer { status, error, .. } => Err(NodeError::new(Failure::Unreachable, status, error)),
  }
}

/// One line to speak on the card the master grants: the voice, the words, where the sound goes, how long the
/// speaking may take once the card is granted, and the demand asked for the card.
pub struct Speech<'a> {
  pub units: &'a Units,
  pub settings: &'a Settings,
  pub voice: &'a VoiceRow,
  pub text: &'a str,
  pub out: &'a Path,
  pub cap_ms: i64,
  pub ask: &'a Ask,
}

/// Where a line stands on its way to a node, as it changes: waiting for its card, with what it waits on, or
/// speaking on the card granted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage<'a> {
  Waiting(&'a str),
  Speaking,
}

/// Which node spoke a line, how long the run waited for its card, and the node's own account of the line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heard {
  pub node: String,
  pub waited_ms: i64,
  pub spoken: Spoken,
}

/// The line spoken on the node of the card the master grants and encoded here into `out`, with nothing laid over
/// the voice (docs/voice-plan.md, 「Which node speaks a line」). The card is waited for however long its grant takes,
/// each stage told as it changes; where the node's card stayed held by other work past the server's own wait, the
/// demand is read again until its card is granted again and the line is asked once more. The demand is given back
/// however the line ends.
pub async fn speak_on_card(
  speech: Speech<'_>,
  told: &mut (dyn FnMut(Stage<'_>) + Send),
) -> Result<Heard, NotSpoken> {
  let started = now_ms();
  let root = speech.settings.voices_folder();
  let request = request(speech.voice, speech.text, &root)?;
  let arbiter = &speech.units.arbiter;
  let ledger = speech.settings.demands_folder();
  let mut held = card::hold(arbiter, &ledger, speech.ask, &mut |why| told(Stage::Waiting(why)))
    .await
    .map_err(NotSpoken::Here)?;

  let ended = loop {
    let waited_ms = now_ms() - started;

    told(Stage::Speaking);

    let call = Call {
      timeout_ms: speech.cap_ms,
      node_wait_ms: None,
    };
    let spoken = match node_shell(speech.units, held.node.clone()).await {
      Ok(shell) => speak_on_node(&shell, &request, call).await,
      Err(error) => Err(error),
    };

    match spoken {
      Ok((wav, spoken)) => {
        break encode(&wav, speech.out, speech.settings)
          .await
          .map_err(NotSpoken::Here)
          .map(|()| Heard {
            node: held.node.clone(),
            waited_ms,
            spoken,
          });
      }
      Err(error) if error.kind == Failure::Taken => {
        match card::again(arbiter, &ledger, &held, speech.ask, &mut |why| told(Stage::Waiting(why))).await {
          Ok(now) => held = now,
          Err(why) => break Err(NotSpoken::Here(why)),
        }
      }
      Err(error) => break Err(NotSpoken::Node(error)),
    }
  };

  card::give_back(arbiter, &ledger, held.demand).await;

  ended
}

// The wav the node spoke written out as OGG/Opus by the voice's encoder entry, the wav taken off the disk however
// the encode ends.
async fn encode(wav: &[u8], out: &Path, settings: &Settings) -> Result<(), String> {
  use std::os::unix::fs::OpenOptionsExt;

  let path = PathBuf::from(format!("{}.node.wav", out.display()));
  let written = std::fs::OpenOptions::new()
    .write(true)
    .create(true)
    .truncate(true)
    .mode(0o600)
    .open(&path)
    .and_then(|mut file| std::io::Write::write_all(&mut file, wav));
  let encoded = match written {
    Err(why) => Err(format!("the node's wav could not be written: {why}")),
    Ok(()) => synth::encode(&path, out, settings)
      .await
      .map_err(|why| format!("the encode step failed: {why}")),
  };

  let _ = std::fs::remove_file(&path);

  encoded
}
