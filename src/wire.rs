//! Every body the voice takes and answers (`docs/boundary-api.md`): the routes answer in these types, the client
//! reads them, the MCP server's tools take and give them, and the OpenAPI document is written from them, so the
//! four never part. The common wire format is bmo-chat's `docs/conventions.md`: camelCase keys, epoch-millisecond
//! times, an error of `code` and `error`, and a page of `items` and `more`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Every failure: a kebab-case code a caller decides on, and an English sentence it decides nothing on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "Error")]
pub struct WireError {
  pub code: String,
  pub error: String,
}

/// Whether the service can answer, and its version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Health {
  pub ready: bool,
  /// Why it is not ready; null while it is.
  pub reason: Option<String>,
  pub version: String,
}

/// One page of a list: its items, and whether anything stands past the last of them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "{T}Page")]
pub struct Page<T> {
  pub items: Vec<T>,
  pub more: bool,
}

/// One event of the service.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Event {
  /// The event number, which only grows.
  pub id: i64,
  /// `synthesis.created`, `synthesis.ended`, `transcription.ended` or `voice.changed`.
  pub kind: String,
  /// When it happened, in epoch milliseconds.
  pub at: i64,
  /// The member as a GET answers it.
  pub data: Value,
}

/// A reading of the event stream, or a page of it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EventQuery {
  /// A comma-separated list of the kinds wanted.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub kinds: Option<String>,
  /// The number of the last event read; the events start past it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// 1 to 1,000; 100 where absent. For a page only.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
}

/// A list's cursor and size.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PageQuery {
  /// The name of the last member read; the page starts past it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<String>,
  /// 1 to 1,000; 100 where absent.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
}

/// One trained voice.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
  /// The voice's name, its key.
  pub name: String,
  /// The language it speaks, as the language data names it.
  pub language: String,
  /// The checkpoint its weights stand at.
  pub checkpoint: String,
  /// The emotions it may be asked to speak with.
  pub emotions: Vec<String>,
  /// Epoch milliseconds of its last change.
  pub changed_at: i64,
}

/// Where a synthesis stands in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SynthesisState {
  /// Taken in, not yet speaking.
  Waiting,
  /// One voice server is speaking it.
  Speaking,
  /// Its sound stands at `/syntheses/{synthesisId}/sound`.
  Done,
  /// It could not be spoken.
  Failed,
}

impl SynthesisState {
  /// The state as the wire and the store write it.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Waiting => "waiting",
      Self::Speaking => "speaking",
      Self::Done => "done",
      Self::Failed => "failed",
    }
  }

  /// The state a word names, or none.
  pub fn parse(word: &str) -> Option<Self> {
    match word {
      "waiting" => Some(Self::Waiting),
      "speaking" => Some(Self::Speaking),
      "done" => Some(Self::Done),
      "failed" => Some(Self::Failed),
      _ => None,
    }
  }
}

/// One synthesis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Synthesis {
  pub id: i64,
  pub voice_name: String,
  pub text: String,
  pub emotion: Option<String>,
  pub state: SynthesisState,
  /// How long its sound runs; null until it is done.
  pub duration_ms: Option<i64>,
  /// Epoch milliseconds.
  pub created_at: i64,
}

/// A text to speak.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewSynthesis {
  pub voice_name: String,
  pub text: String,
  /// One of the voice's emotions; the voice's own where absent.
  #[serde(default)]
  pub emotion: Option<String>,
  /// A delivery laid over the voice, where it names one.
  #[serde(default)]
  pub style: Option<String>,
  #[serde(default)]
  pub seed: Option<i64>,
}

/// One transcription.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Transcription {
  pub id: i64,
  pub language: String,
  /// `waiting`, `running`, `done` or `failed`.
  pub state: String,
  /// What was heard, the files in the order given; null until it is done.
  pub text: Option<String>,
  /// Epoch milliseconds.
  pub created_at: i64,
}

/// Sound files to take down in one language.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewTranscription {
  pub language: String,
  pub files: Vec<String>,
}

/// One speaker encoding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Encoding {
  pub id: i64,
  /// `done` or `failed`.
  pub state: String,
  /// Epoch milliseconds.
  pub created_at: i64,
}

/// A reference recording to encode.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewEncoding {
  /// The wav to encode.
  pub from_wav: String,
}

/// A text read into a script for the ear.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewScriptReading {
  pub text: String,
  /// The language the text is in; the voice's own where absent.
  #[serde(default)]
  pub language: Option<String>,
}

/// A text as a script for the ear: the whole of it, and its sentences.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScriptReading {
  pub script: String,
  pub sentences: Vec<String>,
}

/// One voice server on one node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VoiceServer {
  /// The node's name, its key.
  pub node_name: String,
  /// `running` where the server answers, `absent` where the node does not.
  pub state: String,
  /// The voices whose checkpoints the server holds loaded, in name order.
  pub voice_names: Vec<String>,
}
