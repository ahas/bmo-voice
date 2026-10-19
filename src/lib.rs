//! The voice: briefings spoken in a trained voice, the voice servers on the voice nodes, voice training and the
//! script studio (`docs/layout.md`).

pub mod briefing;
pub mod context;
pub mod policy;
pub mod script;
pub mod settings;
pub mod speech;
pub mod store;
pub mod units;

/// Now, in epoch milliseconds.
pub fn now_ms() -> i64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX))
}
