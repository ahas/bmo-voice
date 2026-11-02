//! The settings every binary reads at start (`docs/layout.md`, 「Settings」): one TOML file, held for the process's
//! life. Every address, path and name of a deployment is read from here, every token from its own file, and every
//! file the voice keeps stands under the data root's `voice/` folder.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The variable naming the settings file, where it is not the one beside the release.
pub const SETTINGS_VARIABLE: &str = "BMO_VOICE_SETTINGS";

/// A unit the voice calls: its base address, and the file under the data root's `env/` its token stands in.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
  pub address: String,
  pub token_file: Option<String>,
}

/// The units the voice calls, each reached only through the routes `docs/boundary-api.md` names.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Units {
  /// How each voice node is reached, and its login.
  pub node: Unit,
  /// The arbiter's master, which grants every synthesis the card it is spoken on.
  pub arbiter: Unit,
  /// The account service the script studio signs a person in against.
  pub web: Unit,
  /// Agency, whose co-worker pool derives a line's spoken words; none where the deployment names none.
  #[serde(default)]
  pub agency: Option<Unit>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
  /// Where the service listens.
  pub listen: String,
  /// The service's own address, as the units it hands work to call it back.
  pub address: String,
  /// The file under the data root's `env/` holding the token callers present.
  pub token_file: String,
  /// The address of the voice's own database.
  pub database: String,
  /// The runtime data root.
  pub data_root: PathBuf,
  /// The folder of the voice's Python side, `synth/`, where it is not the one in the release the running binary
  /// stands in.
  pub synth: Option<PathBuf>,
  pub units: Units,
  /// The script studio's deployment values.
  #[serde(default)]
  pub studio: Studio,
  /// How a line's spoken words are derived.
  #[serde(default)]
  pub spoken: Spoken,
}

/// How a line's spoken words are derived on agency's co-worker pool (`docs/boundary-api.md`, 「How the engine
/// derives the spoken words」).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Spoken {
  /// The agency role the slot is asked for.
  pub role: String,
  /// The runner line the derivation's session goes out on.
  pub line: String,
  /// The provider the slot is asked on, where the deployment names one; named together with `model`.
  pub provider: Option<String>,
  /// The model the slot is asked on, where the deployment names one; named together with `provider`.
  pub model: Option<String>,
}

impl Default for Spoken {
  fn default() -> Self {
    Self {
      role: "transliterator".to_owned(),
      line: "search".to_owned(),
      provider: None,
      model: None,
    }
  }
}

/// What the script studio holds of a deployment (`docs/script-studio.md`, 「Access and service」).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Studio {
  /// The folder the source manifest's paths and the download records' keys stand under, which an asset's path is
  /// read without; none where they stand at the top.
  pub source_root: Option<String>,
  /// The address an export names as its source.
  pub source_url: Option<String>,
  /// The spellings of the speaker label a subtitle cue may open with, which matching drops; the first is the speaker
  /// the media tools name in an automatic line's id and notes.
  #[serde(default)]
  pub speakers: Vec<String>,
  /// The address the media tools download the source material from, read by them alone.
  pub cloud: Option<String>,
  /// The folder of the manifest's paths the cloud's player names its videos below, read by the media tools alone.
  pub cloud_home: Option<String>,
}

/// Why the settings would not stand.
#[derive(Debug)]
pub struct Unread(pub String);

impl std::fmt::Display for Unread {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(&self.0)
  }
}

impl std::error::Error for Unread {}

impl Settings {
  /// The settings a file holds.
  pub fn read(path: &Path) -> Result<Self, Unread> {
    let text = std::fs::read_to_string(path).map_err(|why| Unread(format!("{}: {why}", path.display())))?;
    let settings: Self =
      toml::from_str(&text).map_err(|why| Unread(format!("{}: {why}", path.display())))?;

    // A slot's road names its provider and its model together, or neither, the role's order picking then.
    if settings.spoken.provider.is_some() != settings.spoken.model.is_some() {
      return Err(Unread(format!(
        "{}: [spoken] names provider and model together, or neither",
        path.display()
      )));
    }

    Ok(settings)
  }

  /// Where the settings stand: the file the variable names, else `settings.toml` at the top of the release the
  /// running binary stands in.
  pub fn path() -> Result<PathBuf, Unread> {
    if let Some(named) = std::env::var_os(SETTINGS_VARIABLE) {
      return Ok(PathBuf::from(named));
    }

    let binary = std::env::current_exe().map_err(|why| Unread(format!("the running binary: {why}")))?;
    let release = binary.parent().and_then(Path::parent).ok_or_else(|| {
      Unread(format!(
        "{} stands in no release; name the settings in {SETTINGS_VARIABLE}",
        binary.display()
      ))
    })?;

    Ok(release.join("settings.toml"))
  }

  /// The settings where they stand.
  pub fn load() -> Result<Self, Unread> {
    Self::read(&Self::path()?)
  }

  /// The folder of the voice's Python side: the one the settings name, else `synth/` at the top of the release the
  /// running binary stands in.
  pub fn synth_folder(&self) -> Result<PathBuf, Unread> {
    if let Some(named) = &self.synth {
      return Ok(named.clone());
    }

    Ok(Self::release_folder()?.join("synth"))
  }

  /// The top of the release the running binary stands in, `bin/` beneath it.
  pub fn release_folder() -> Result<PathBuf, Unread> {
    let binary = std::env::current_exe().map_err(|why| Unread(format!("the running binary: {why}")))?;

    binary
      .parent()
      .and_then(Path::parent)
      .map(Path::to_path_buf)
      .ok_or_else(|| Unread(format!("{} stands in no release", binary.display())))
  }

  /// The voice's own folder under the data root, where everything it keeps stands.
  pub fn voice_folder(&self) -> PathBuf {
    self.data_root.join("voice")
  }

  /// The installed voices (`weights/checkpoints.toml`).
  pub fn voices_folder(&self) -> PathBuf {
    self.voice_folder().join("voices")
  }

  /// The locks the voice takes: the synthesis on this machine's card, and one per voice node.
  pub fn locks_folder(&self) -> PathBuf {
    self.voice_folder().join("locks")
  }

  /// The ledger of the demands of the arbiter's master the service holds, one file each.
  pub fn demands_folder(&self) -> PathBuf {
    self.voice_folder().join("demands")
  }

  /// The token the voice servers on the voice nodes check.
  pub fn node_token_file(&self) -> PathBuf {
    self.voice_folder().join("voice.token")
  }

  /// A file under the data root's `env/`.
  pub fn env_file(&self, name: &str) -> PathBuf {
    self.data_root.join("env").join(name)
  }

  /// The token a file under `env/` holds, trimmed; none where it holds none.
  pub fn token(&self, name: &str) -> Result<Option<String>, Unread> {
    let path = self.env_file(name);
    let text =
      std::fs::read_to_string(&path).map_err(|why| Unread(format!("{}: {why}", path.display())))?;
    let token = text.trim();

    Ok((!token.is_empty()).then(|| token.to_owned()))
  }

  /// The token a unit is called with, where it names a file.
  pub fn unit_token(&self, unit: &Unit) -> Result<Option<String>, Unread> {
    match &unit.token_file {
      Some(name) => self.token(name),
      None => Ok(None),
    }
  }
}
