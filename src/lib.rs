//! The voice engine: trained voices, speech synthesis on the voice servers of the voice nodes, transcription,
//! speaker encodings, script readings, and the script studio (`docs/layout.md`).

pub mod client;
pub mod context;
pub mod events;
pub mod http;
pub mod import;
pub mod mcp;
pub mod narration;
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

/// The version this build answers with: the set's, which the testbed hands the build as `BMO_VERSION`, else
/// the manifest's placeholder (bmo-chat's `docs/conventions.md`, 「Versions」).
pub const VERSION: &str = match option_env!("BMO_VERSION") {
  Some(version) => version,
  None => env!("CARGO_PKG_VERSION"),
};

/// Now, in epoch milliseconds.
pub fn now_ms() -> i64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_or(0, |since| i64::try_from(since.as_millis()).unwrap_or(i64::MAX))
}
