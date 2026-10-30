//! The tool host's side of a call (bmo-pocket's `docs/mcp-host.md`, 「A call」): the one request the
//! executable is handed on standard input and the tool result it answers on standard output.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The version of the call protocol this tool speaks.
pub const PROTOCOL: u32 = 1;

/// What the executable is asked.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Asked {
  /// One entry, by its full name, called with its arguments; answered with a tool result.
  Call {
    entry: String,
    #[serde(default)]
    arguments: Value,
  },
  /// The resources it serves, of which it has none.
  ResourcesList,
  /// One resource, which it cannot read, since it serves none.
  ResourcesRead { uri: String },
  /// A kind this tool does not answer: a claim, a helper's start, a server's start.
  #[serde(other)]
  Other,
}

/// Everything the executable is handed: the protocol version, what it is asked and the session record it is
/// asked under. The record and the sockets beside it are passed over: the tool calls nothing back.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Handed {
  pub protocol: u32,
  #[serde(flatten)]
  pub asked: Asked,
  #[serde(default)]
  pub session: Value,
}

/// The request read off `input`, or why it does not read: an object that is no request, or one in a
/// protocol version this tool does not speak.
pub fn read_handed(input: impl std::io::Read) -> Result<Handed, String> {
  let handed: Handed =
    serde_json::from_reader(input).map_err(|why| format!("the request does not read: {why}"))?;

  if handed.protocol != PROTOCOL {
    return Err(format!(
      "the request is in protocol {}; this tool speaks protocol {PROTOCOL}",
      handed.protocol
    ));
  }

  Ok(handed)
}

/// One piece of an answer: the tool answers text alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
  Text { text: String },
}

/// The protocol's tool result. A refusal is an answer marked as an error, which the session reads as it
/// reads any other.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
  pub content: Vec<Content>,
  #[serde(rename = "isError", default)]
  pub is_error: bool,
}

impl Answer {
  pub fn text(text: impl Into<String>) -> Self {
    Self {
      content: vec![Content::Text { text: text.into() }],
      is_error: false,
    }
  }

  /// An answer marked as an error, saying why.
  pub fn error(text: impl Into<String>) -> Self {
    Self {
      is_error: true,
      ..Self::text(text)
    }
  }

  /// The text of every piece, joined, as a reader of text alone reads the answer.
  pub fn plain(&self) -> String {
    self
      .content
      .iter()
      .map(|Content::Text { text }| text.as_str())
      .collect::<Vec<_>>()
      .join("\n")
  }
}
