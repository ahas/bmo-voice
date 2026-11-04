//! The voice servers read for a session (docs/boundary-api.md, 「The hosted tool」): the settings found, every
//! page of `GET /voice-servers` called through the engine's own client, and the list said as text with where a
//! line is spoken now (docs/voice-plan.md, 「Which road speaks a line」).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use bmo_voice::client::{ClientFailure, VoiceClient};
use bmo_voice::mcp::ToolFailure;
use bmo_voice::settings::{SETTINGS_VARIABLE, Settings};
use bmo_voice::wire::{PageQuery, VoiceServer};

use crate::call::Answer;

/// The settings file where the variable names none: the one under the link of the voice's deploy tree, below the
/// home, since the tool host runs the registry's copy of the placed folder, which stands in no release.
pub const DEPLOY_SETTINGS: &str = ".local/share/bmo-voice/deploy/settings.toml";

/// The most servers one page is asked for.
const PAGE_LIMIT: u32 = 1000;

/// Where the settings stand: the file `BMO_VOICE_SETTINGS` names, else the deploy tree's under `home`, or why
/// they stand nowhere. A variable set to nothing names no file.
pub fn settings_path(named: Option<OsString>, home: Option<OsString>) -> Result<PathBuf, String> {
  if let Some(named) = named.filter(|named| !named.is_empty()) {
    return Ok(PathBuf::from(named));
  }

  match home.filter(|home| !home.is_empty()) {
    Some(home) => Ok(Path::new(&home).join(DEPLOY_SETTINGS)),
    None => Err(format!(
      "neither {SETTINGS_VARIABLE} nor HOME is set, so the voice's settings stand nowhere"
    )),
  }
}

/// The settings path of this process's own environment.
pub fn settings_of_process() -> Result<PathBuf, String> {
  settings_path(std::env::var_os(SETTINGS_VARIABLE), std::env::var_os("HOME"))
}

/// Every voice server in the node list's order, page after page.
pub async fn listed(client: &VoiceClient) -> Result<Vec<VoiceServer>, ClientFailure> {
  let mut servers = Vec::new();
  let mut after = None;

  loop {
    let page = client
      .voice_servers(&PageQuery {
        after,
        limit: Some(PAGE_LIMIT),
      })
      .await?;

    after = page.items.last().map(|server| server.node_name.clone());
    servers.extend(page.items);

    if !page.more || after.is_none() {
      return Ok(servers);
    }
  }
}

/// The list said as text: one line a voice server, its node's name, its state and the voices it holds loaded,
/// then where a line is spoken now.
pub fn servers_text(servers: &[VoiceServer]) -> String {
  let mut lines: Vec<String> = servers
    .iter()
    .map(|server| {
      let voices = if server.voice_names.is_empty() {
        "none".to_owned()
      } else {
        server.voice_names.join(", ")
      };

      format!("{}: {}; voices loaded: {voices}", server.node_name, server.state)
    })
    .collect();

  lines.push(if servers.is_empty() {
    "Where a line is spoken now: nowhere. No voice node is listed, so a synthesis fails, and nothing speaks on \
     the operations machine's card."
      .to_owned()
  } else {
    "Where a line is spoken now: on the voice server of the listed node the arbiter's master grants a card of voice."
      .to_owned()
  });

  lines.join("\n")
}

/// A call that came to no answer of the route's shape, said as the harness plugin's failures are: the status,
/// code and sentence of the refusal, or the sentence saying why nothing answered.
pub fn failure_text(failure: &ClientFailure) -> String {
  ToolFailure::Client(failure.clone()).content().to_string()
}

/// The engine's client built from the settings at `settings`, or why it stands nowhere: settings or a token that
/// do not read, said as the call's answer.
pub fn client_of(settings: &Path) -> Result<VoiceClient, String> {
  let settings =
    Settings::read(settings).map_err(|why| format!("the voice's settings do not read: {why}"))?;

  settings
    .token(&settings.token_file)
    .map_err(|why| format!("the voice's token does not read: {why}"))
    .and_then(|token| VoiceClient::new(&settings.address, token).map_err(|why| failure_text(&why)))
}

/// The voice servers read with the settings at `settings`, as the call's answer.
pub async fn servers(settings: &Path) -> Answer {
  let client = match client_of(settings) {
    Ok(client) => client,
    Err(why) => return Answer::error(why),
  };

  match listed(&client).await {
    Ok(servers) => Answer::text(servers_text(&servers)),
    Err(failure) => Answer::error(failure_text(&failure)),
  }
}
