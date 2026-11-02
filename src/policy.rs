//! The voice's own policy values (`policy/voice.toml`), read once.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

const SOURCE: &str = include_str!("../policy/voice.toml");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoicePolicy {
  /// How long one line may take to speak, in milliseconds, from the grant of its card to its sound, before the
  /// time its words add.
  pub run_cap_ms: i64,
  /// What each character of a line's spoken words adds to how long it may take, in milliseconds.
  pub run_ms_per_character: i64,
  /// The emotions a voice may be asked to speak with.
  pub emotions: Vec<String>,
  /// The engine every served voice belongs to.
  pub engine: String,
}

impl VoicePolicy {
  /// How long a line of these spoken words may take to speak once its card is granted, in milliseconds
  /// (docs/voice-plan.md, 「What a long line takes of the card」).
  pub fn run_cap_for(&self, spoken: &str) -> i64 {
    let characters = i64::try_from(spoken.chars().count()).unwrap_or(i64::MAX);

    self.run_cap_ms.saturating_add(characters.saturating_mul(self.run_ms_per_character))
  }
}

/// The policy, read once.
pub static POLICY: LazyLock<VoicePolicy> = LazyLock::new(|| {
  toml::from_str(SOURCE).unwrap_or_else(|why| panic!("policy/voice.toml does not read: {why}"))
});
