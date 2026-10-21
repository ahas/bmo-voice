//! The voice engine: trained voices, speech synthesis on the voice servers of the voice nodes, transcription,
//! speaker encodings, script readings, and the script studio (`docs/layout.md`).

pub mod client;
pub mod context;
pub mod events;
pub mod http;
pub mod import;
pub mod mcp;
pub mod openapi;
pub mod policy;
pub mod script;
pub mod service;
pub mod settings;
pub mod speech;
pub mod store;
pub mod studio;
pub mod units;
pub mod wire;

/// Now, in epoch milliseconds.
pub fn now_ms() -> i64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX))
}
