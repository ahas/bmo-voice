//! The voice's own policy values (`policy/voice.toml`), read once.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

const SOURCE: &str = include_str!("../policy/voice.toml");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoicePolicy {
  /// The emotions a voice may be asked to speak with.
  pub emotions: Vec<String>,
  /// The engine every served voice belongs to.
  pub engine: String,
  /// The spans of the clocks that end a synthesis.
  pub clocks: ClockSpans,
}

/// The spans of the three clocks that end a synthesis nothing else would end (`docs/boundary-api.md`, 「The clocks」),
/// in whole seconds.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClockSpans {
  /// How long a synthesis its caller has not read waits before it fails.
  pub caller_gone_seconds: u64,
  /// How long one call to reach a line's node may take.
  pub node_answer_seconds: u64,
  /// How long a line on its node may stand still.
  pub node_still_seconds: u64,
}

/// The policy, read once.
pub static POLICY: LazyLock<VoicePolicy> = LazyLock::new(|| {
  toml::from_str(SOURCE).unwrap_or_else(|why| panic!("policy/voice.toml does not read: {why}"))
});
