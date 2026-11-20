//! A line spoken on a voice node (docs/voice-plan.md, 「The voice servers on the voice nodes」, 「Which node speaks
//! a line」, 「How a line crosses」, 「When the nodes are out of reach」). The server listens on the node's loopback
//! alone, so every call goes through the node's own shell: the request handed in on standing input, the asker
//! started apart on the node, its answer and the line's stage asked after until it is there, the wav carried back
//! and every file of the call removed, the asker ended first where the call is let go of before it. Every command
//! answers within the node's answer clock, and a line standing still on the node ends on its progress clock
//! (docs/boundary-api.md, 「The clocks」). The OGG/Opus encode stays on this machine.
//!
//! Which node speaks a line is the card the arbiter's master grants it ([`super::card`]); how that node is reached is
//! the node service's answer for it. The card is held for the whole of the line, and a node out of reach is never
//! quietly replaced by this machine's card.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use super::{card, shell_word, synth};
use crate::context::Clocks;
use crate::now_ms;
use crate::script::reading::Language;
use crate::settings::Settings;
use crate::store::voices::VoiceRow;
use crate::units::Units;
use crate::units::arbiter::Ask;
use crate::units::hub::{ExecEnd, HubTarget, hub_exec};
use crate::units::node::Reach;

/// The voice's own name: the service the arbiter's master grants its cards of, and the one a node's row may reserve
/// the node for.
pub const SERVICE: &str = "voice";

/// The start of every synthesis's work key, `voice:synthesis:<synthesisId>`: the key its demand is asked with.
pub const WORK: &str = "voice:synthesis:";

/// The work key a synthesis's demand for its card is asked with.
pub fn work_key(synthesis: i64) -> String {
  format!("{WORK}{synthesis}")
}

// Where everything lands on the node, relative to its home, the port the server listens on there, and the token
// file beside the node's other configuration.
const PLACE: &str = "bmo-voice";
const PORT: u16 = 7432;
const TOKEN: &str = ".config/bmo-voice/token";

// The start of a call's command exits with this where the server is not installed on the node.
const NOT_INSTALLED: i32 = 4;

// The engine environment's interpreter under the node's place (`docs/voice-plan.md`, 「The environments」).
const ENGINE_PYTHON: &str = "fish-speech/.venv/bin/python";

// The longest one wait on the node lasts, a piece well inside what a hub node lets one command run; the wait is
// asked again until the asker lets go.
const ASK_WAIT_SECONDS: i64 = 10;

// What every call sweeps from the node's call folder first: the files of a call whose asker has let go of its lock
// and of which nothing has been written for an hour, and any file an hour old with no lock of its own beside it. A
// call whose asker still holds its lock is never swept, however long it runs; a fresh call is passed over before its
// lock is even tried, so the sweep never stands in the way of a call starting.
const SWEEP: &str = r#"for l in "$d"/*.lock; do [ -e "$l" ] || continue; c="${l%.lock}"; [ -z "$(find "$d" -maxdepth 1 -name "${c##*/}.*" -mmin -60 -print -quit)" ] && flock -n "$l" true 2>/dev/null && rm -f "$c".*; done
find "$d" -maxdepth 1 -type f ! -name "*.lock" -mmin +60 | while IFS= read -r f; do [ -e "${f%.*}.lock" ] || rm -f "$f"; done"#;

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

/// Why a line was not spoken on a node: a request no node could take, the failure of the node granted, named, or a
/// failure here: the master's refusal of its card or this machine's own encode step.
#[derive(Debug)]
pub enum NotSpoken {
  Node(NodeError),
  OnNode(String, NodeError),
  Here(String),
}

impl fmt::Display for NotSpoken {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Node(error) => error.fmt(f),
      Self::OnNode(node, error) => write!(f, "on {node}: {error}"),
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

/// How a node's own shell is reached: an ssh node over a batch login, a hub node through its hub; a node whose login
/// answers in PowerShell runs no bash command and is out of reach.
#[derive(Clone)]
enum Reached {
  Ssh { args: Vec<String>, host: String },
  Hub(HubTarget),
  PowerShell(String),
}

/// A node's own shell: a command run there as bash runs it, with bytes on its standing input, each command answering
/// within the node's answer clock or failing as the node not answering, and a line on it ending on its progress
/// clock.
#[derive(Clone)]
pub struct NodeShell {
  reached: Reached,
  clocks: Clocks,
}

impl NodeShell {
  // A failure to reach the node at all, or a command it did not answer within the clock, as the call's error; a
  // command that ran and failed is a run.
  async fn run(&self, command: &str, input: Option<&[u8]>) -> Result<ShellRun, String> {
    let within = self.clocks.node_answer;

    self
      .clocks
      .within(within, self.reached.run(command, input))
      .await
      .unwrap_or_else(|| Err(format!("it did not answer within {}", span(within))))
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

impl Reached {
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
}

fn tail(text: &str, most: usize) -> String {
  let count = text.chars().count();

  text.chars().skip(count.saturating_sub(most)).collect()
}

/// A span as a person reads it: whole seconds where it is whole, tenths otherwise.
pub fn span(span: Duration) -> String {
  if span.subsec_millis() == 0 {
    format!("{} s", span.as_secs())
  } else {
    format!("{:.1} s", span.as_secs_f64())
  }
}

/// The shell of a node by its name, as the node service says it is reached within the node service's clock, every
/// command on it answering within the node's answer clock, both counted on `clocks`. A name the service does not
/// list, a node that cannot be reached as it stands, a node its role reserves for other services, and a node service
/// that does not answer within its clock are all the node out of reach.
pub async fn node_shell(units: &Units, name: String, clocks: &Clocks) -> Result<NodeShell, NodeError> {
  let service_within = clocks.node_service;
  let silent =
    || unreachable(format!("the node service did not answer for {name} within {}", span(service_within)));
  let target = clocks
    .within(service_within, units.node.target(name.clone()))
    .await
    .ok_or_else(silent)?
    .map_err(|why| unreachable(format!("there is no reachable node called {name}: {why}")))?;

  if let Some(role) =
    target.role.as_deref().filter(|role| !role.split('+').any(|service| service == SERVICE))
  {
    return Err(unreachable(format!("{name} is reserved for {role} and carries no {SERVICE}")));
  }

  let reached = match target.reach {
    Reach::Hub {
      hub_url,
      token_file,
      agent,
    } => Reached::Hub(HubTarget {
      hub_url,
      agent,
      token_file,
    }),
    Reach::Ssh {
      host, port, shell, ..
    } if shell.as_deref() == Some("powershell") => Reached::PowerShell(format!("{host}:{port}")),
    Reach::Ssh { host, .. } => {
      let args = clocks
        .within(service_within, units.node.login(name.clone()))
        .await
        .ok_or_else(silent)?
        .map_err(|why| unreachable(why.to_string()))?
        .ok_or_else(|| unreachable(format!("{name} has no SSH route")))?;

      Reached::Ssh { args, host }
    }
  };

  Ok(NodeShell {
    reached,
    clocks: clocks.clone(),
  })
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
/// node's copy shares, with the seed, the delivery and the sentence switch the row speaks with, and the language the
/// line is said in, which picks its delivery clip (`docs/voice-plan.md`, 「What it answers」).
pub fn request(voice: &VoiceRow, text: &str, language: Language, root: &Path) -> Result<Value, NodeError> {
  let checkpoint = under(root, &voice.checkpoint).ok_or_else(|| {
    NodeError::new(
      Failure::Elsewhere,
      None,
      Some(format!("{} is not under {}", voice.name, root.display())),
    )
  })?;

  Ok(json!({
    "text": text, "checkpoint": checkpoint, "seed": voice.seed, "style": voice.style, "sentences": voice.by_sentence,
    "language": language.code(),
  }))
}

// The node's interpreter running the server file, as a line of the node's shell.
fn on_node() -> String {
  format!(r#""$HOME/{PLACE}/{ENGINE_PYTHON}" "$HOME/{PLACE}/voice-fish.py""#)
}

// The folder a call's files stand in on the node, as a line of the node's shell.
fn call_folder() -> String {
  format!(r#"d="$HOME/{PLACE}/spoken""#)
}

// Starts the asker apart from the call, in a process group of its own whose id stands beside the call's files, and
// says where those files stand. The call's lock is taken before any of its files is written, and the asker holds it
// from before this returns until it has written its end, so the moment it lets go is the moment its line is there.
// The asker hands the server the call's stage file, where the server writes where the line stands.
fn start_command(id: &str) -> String {
  let asker = on_node();

  [
    call_folder(),
    r#"mkdir -p "$d" && chmod 700 "$d" || exit 3"#.to_owned(),
    SWEEP.to_owned(),
    format!(
      r#"[ -x "$HOME/{PLACE}/{ENGINE_PYTHON}" ] || {{ echo "the voice server is not installed on this node" >&2; exit {NOT_INSTALLED}; }}"#
    ),
    "umask 077".to_owned(),
    format!(r#"exec 9> "$d/{id}.lock" && flock -n 9 || exit 3"#),
    format!(r#"cat > "$d/{id}.in" || exit 3"#),
    format!(
      r#"setsid sh -c '{asker} --ask --port {PORT} --token-file "$HOME/{TOKEN}" --request "$1/$2.in" --wav "$1/$2.wav" --stage "$1/$2.stage" > "$1/$2.out" 2> "$1/$2.err"; echo $? > "$1/$2.code"' sh "$d" {} < /dev/null > /dev/null 2>&1 &"#,
      shell_word(id)
    ),
    format!(r#"echo $! > "$d/{id}.pid""#),
    "exec 9>&-".to_owned(),
    r#"printf '%s\n' "$d""#.to_owned(),
  ]
  .join("\n")
}

// Ends a call on the node: where `asker` holds, the asker's process group is ended first, so the server sees its line
// let go; then every file of the call is removed.
fn end_command(id: &str, asker: bool) -> String {
  let id = shell_word(id);
  let mut lines = vec![call_folder()];

  if asker {
    lines.push(format!(r#"[ -f "$d/"{id}.pid ] && kill -TERM -- -"$(cat "$d/"{id}.pid)" 2>/dev/null"#));
  }

  lines.push(format!(r#"rm -f "$d/"{id}.*"#));
  lines.join("\n")
}

// Waits on the node, `seconds` at most, for the asker to let go of the call's lock, then says whether it has
// ended and what it wrote: `waiting` followed by the line's stage as the server last wrote it and the moment any
// line's stage in the folder last changed, `gone` where it ended without writing its end, or `done <code>` followed
// by its answer and the tail of its error stream folded onto one line.
fn await_command(dir: &str, id: &str, seconds: i64) -> String {
  let folder = shell_word(dir);
  let at = format!("{folder}/{}", shell_word(id));

  [
    format!("[ -f {at}.code ] || flock -w {seconds} -s {at}.lock true 2>/dev/null"),
    format!("if flock -n -s {at}.lock true 2>/dev/null; then free=1; else free=0; fi"),
    format!(
      r#"if [ -f {at}.code ]; then echo "done $(cat {at}.code)"; cat {at}.out; echo; tail -c 1000 {at}.err | tr '\n' ' '; elif [ $free = 1 ]; then echo gone; else echo waiting; [ -f {at}.stage ] && printf 'stage %s\n' "$(head -c 2000 {at}.stage | tr '\n' ' ')"; printf 'moved %s\n' "$(find {folder} -maxdepth 1 -name '*.stage' -printf '%T@\n' 2>/dev/null | sort -n | tail -n 1)"; fi"#
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
  load_parts: Option<String>,
  checkpoint: Option<String>,
}

fn failure_of(asked: &Asked) -> NodeError {
  let error = asked.error.clone();

  let Some(status) = asked.status else {
    let kind = match error.as_deref() {
      Some(error) if error.starts_with("token") => Failure::Refused,
      _ => Failure::Unreachable,
    };

    return NodeError::new(kind, None, error);
  };

  // The server's 503 is the node's card arbiter giving the voice no place on the card: the line did not get its
  // turn there, and its demand is read again.
  let kind = match status {
    503 => Failure::Taken,
    500.. => Failure::Failed,
    _ => Failure::Refused,
  };

  NodeError::new(kind, Some(status), error)
}

/// Where a line stands on its voice server, as the server writes it into the call's stage file: the stage's kind,
/// its count of steps, which grows with every change, and an English sentence saying it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct Staged {
  pub stage: String,
  #[serde(default)]
  pub step: u64,
  #[serde(default)]
  pub said: String,
}

// What one look at a waiting call read: the line's stage, where the server wrote one, and the moment any line of the
// server last moved, as the node's clock writes it.
#[derive(Default, PartialEq, Eq)]
struct Look {
  stage: Option<Staged>,
  moved: String,
}

fn look_of(lines: &[&str]) -> Look {
  let mut look = Look::default();

  for line in lines {
    if let Some(stage) = line.strip_prefix("stage ") {
      look.stage = serde_json::from_str(stage.trim()).ok();
    } else if let Some(moved) = line.strip_prefix("moved ") {
      moved.trim().clone_into(&mut look.moved);
    }
  }

  look
}

// Waits on the node, a piece at a time, until the asker has ended: what it wrote, or how it ended without it. Each
// stage the server writes for the line is handed to `told` as it changes; a line whose stage and whose server's
// every line stand still for the node's progress clock fails, naming where it stood. A move is counted before it is
// told, so whoever hears of it finds the progress clock already started again.
async fn ask_after(
  shell: &NodeShell,
  dir: &str,
  id: &str,
  told: &mut (dyn FnMut(&Staged) + Send),
) -> Result<Asked, NodeError> {
  let clocks = &shell.clocks;
  let still = clocks.node_still;
  let mut seen = Look::default();
  let mut moved_at = clocks.now();

  loop {
    let polled = shell.run(&await_command(dir, id, ASK_WAIT_SECONDS), None).await.map_err(unreachable)?;
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

    let look = look_of(&lines[1..]);

    if look != seen {
      moved_at = clocks.now();

      if let Some(stage) = look.stage.as_ref().filter(|stage| seen.stage.as_ref() != Some(*stage)) {
        told(stage);
      }

      seen = look;
    } else if clocks.now() - moved_at >= still {
      let at = seen
        .stage
        .as_ref()
        .map_or("before the voice server wrote any stage", |stage| stage.said.as_str());

      return Err(NodeError::new(
        Failure::Failed,
        None,
        Some(format!("the line stood still for {} at: {at}", span(still))),
      ));
    }
  }
}

/// The node's own account of a line it spoke.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spoken {
  pub seconds: f64,
  pub load_seconds: f64,
  pub speak_seconds: f64,
  /// What the load and the line's first capture took part by part, in the node's own words; empty where it says
  /// none.
  pub load_parts: String,
  pub checkpoint: String,
}

// One call on a node, ended there however its speaking ends: where it is let go of before its asker ended, dropped by
// a clock, a stop or a failure, the asker's process group is ended and the call's files removed, on a task of its
// own, since nothing waits on that.
struct Call {
  shell: NodeShell,
  id: String,
  ended: bool,
}

impl Call {
  // The asker ended by itself: only its files are left to remove.
  async fn end(mut self) {
    self.ended = true;

    let _ = self.shell.run(&end_command(&self.id, false), None).await;
  }
}

impl Drop for Call {
  fn drop(&mut self) {
    if self.ended {
      return;
    }

    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
      return;
    };
    let shell = self.shell.clone();
    let command = end_command(&self.id, true);

    runtime.spawn(async move {
      let _ = shell.run(&command, None).await;
    });
  }
}

// One line spoken on the node, as the wav it answered, waited for until the asker on the node has ended or the line
// stands still for the node's progress clock; each stage the server writes is handed to `told`.
async fn speak_on_node(
  shell: &NodeShell,
  request: &Value,
  told: &mut (dyn FnMut(&Staged) + Send),
) -> Result<(Vec<u8>, Spoken), NodeError> {
  let id = format!("{:x}-{:08x}", now_ms(), rand::random::<u32>());
  let call = Call {
    shell: shell.clone(),
    id: id.clone(),
    ended: false,
  };
  let input = request.to_string();
  let begun = shell.run(&start_command(&id), Some(input.as_bytes())).await.map_err(unreachable)?;

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

  let asked = ask_after(shell, &named, &id, told).await?;

  told(&staged(CARRIED, "its sound is carried back"));

  // The asker has ended: the line and its sound leave the node with the call, however what it wrote reads.
  let spoken = carried(shell, &named, &id, &asked).await;

  call.end().await;

  spoken
}

// The wav the asker wrote, carried back once its answer says the line was spoken.
async fn carried(
  shell: &NodeShell,
  dir: &str,
  id: &str,
  asked: &Asked,
) -> Result<(Vec<u8>, Spoken), NodeError> {
  if asked.status != Some(200) {
    return Err(failure_of(asked));
  }

  let wav = shell.get(&format!("{dir}/{id}.wav")).await.map_err(unreachable)?;

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
      load_parts: asked.load_parts.clone().unwrap_or_default(),
      checkpoint: asked.checkpoint.clone().unwrap_or_default(),
    },
  ))
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

/// The node's server asked what it holds, its answer waited for. A node that cannot be asked, or a server that is
/// not listening, is out of reach.
pub async fn health(shell: &NodeShell) -> Result<Health, NodeError> {
  let ran = shell
    .run(&format!(r#"{} --ask --port {PORT} --token-file "$HOME/{TOKEN}""#, on_node()), None)
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

/// One line to speak on the card the master grants: the voice, the words and their language, where the sound goes, the demand asked
/// for the card, and the clocks of the node and the clock they are counted on: how long the node service may take to
/// say how the node is reached, how long one command on the node may take and how long the line may stand still on it.
pub struct Speech<'a> {
  pub units: &'a Units,
  pub settings: &'a Settings,
  pub voice: &'a VoiceRow,
  pub text: &'a str,
  pub language: Language,
  pub out: &'a Path,
  pub ask: &'a Ask,
  pub clocks: &'a Clocks,
}

/// Where a line stands on its way to a node, as it changes: waiting for its card, with what it waits on, or on the
/// node of the card granted, held by its demand, with where the line stands there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage<'a> {
  Waiting(&'a str),
  OnNode {
    demand: i64,
    node: &'a str,
    stage: &'a Staged,
  },
}

/// The line asked of its node, before the server has written any stage of it.
pub const ASKED: &str = "asked";

/// The line spoken, its sound carried back from the node.
pub const CARRIED: &str = "carried";

fn staged(stage: &str, said: &str) -> Staged {
  Staged {
    stage: stage.to_owned(),
    step: 0,
    said: said.to_owned(),
  }
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
/// each stage told as it changes, and the speaking while the node answers and the line moves on it, each stage the
/// server writes told too; where the node's card arbiter gives the voice no place on the card, the demand is read
/// again until its card is granted again and the line is asked once more. The demand is given back however the line
/// ends.
pub async fn speak_on_card(
  speech: Speech<'_>,
  told: &mut (dyn FnMut(Stage<'_>) + Send),
) -> Result<Heard, NotSpoken> {
  let started = now_ms();
  let root = speech.settings.voices_folder();
  let request = request(speech.voice, speech.text, speech.language, &root)?;
  let arbiter = &speech.units.arbiter;
  let ledger = speech.settings.demands_folder();
  let mut held = card::hold(arbiter, &ledger, speech.ask, &mut |why| told(Stage::Waiting(why)))
    .await
    .map_err(NotSpoken::Here)?;

  let ended = loop {
    let waited_ms = now_ms() - started;
    let (demand, node) = (held.demand, held.node.clone());
    let mut on_node = |stage: &Staged| {
      told(Stage::OnNode {
        demand,
        node: &node,
        stage,
      })
    };

    on_node(&staged(ASKED, "the line is asked of the node"));

    let spoken = match node_shell(speech.units, node.clone(), speech.clocks).await {
      Ok(shell) => speak_on_node(&shell, &request, &mut on_node).await,
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
      Err(error) => break Err(NotSpoken::OnNode(held.node.clone(), error)),
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
