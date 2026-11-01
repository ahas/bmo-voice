//! The voice's own policy values (`policy/voice.toml`), read once.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

const SOURCE: &str = include_str!("../policy/voice.toml");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoicePolicy {
  /// How long one line may take to speak, in milliseconds, from the grant of its card to its sound.
  pub run_cap_ms: i64,
  /// The emotions a voice may be asked to speak with.
  pub emotions: Vec<String>,
  /// The engine every served voice belongs to.
  pub engine: String,
}

/// The policy, read once.
pub static POLICY: LazyLock<VoicePolicy> = LazyLock::new(|| {
  toml::from_str(SOURCE).unwrap_or_else(|why| panic!("policy/voice.toml does not read: {why}"))
});
