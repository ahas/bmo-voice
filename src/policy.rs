//! The voice's own policy values (`policy/voice.toml`), read once.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

const SOURCE: &str = include_str!("../policy/voice.toml");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoicePolicy {
  /// How long a briefing's run may take, in milliseconds, before its clock asks whether to keep it.
  pub run_cap_ms: i64,
  /// The kind a notice of a briefing that did not go out is queued under.
  pub failed_kind: String,
  /// The emotions a briefing may be said with.
  pub emotions: Vec<String>,
}

/// The policy, read once.
pub static POLICY: LazyLock<VoicePolicy> = LazyLock::new(|| {
  toml::from_str(SOURCE).unwrap_or_else(|why| panic!("policy/voice.toml does not read: {why}"))
});
