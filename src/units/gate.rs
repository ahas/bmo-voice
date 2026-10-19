//! bmo-gate, as its `docs/boundary-api.md` states it (「For the voice unit: the voice gates」, 「For the console」):
//! one run of a gate, and a gate's options. Bodies are snake_case.

use reqwest::Method;
use serde::{Deserialize, Serialize};

use super::{Caller, UnitError};
use crate::script::{GateError, GateRequest};

/// The gate unit.
#[derive(Clone, Debug)]
pub struct Gate {
  caller: Caller,
}

#[derive(Serialize)]
struct Run<'a> {
  prompt: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  chat: Option<i64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  message: Option<i64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  agent: Option<&'a str>,
  thinking: bool,
  alongside: bool,
}

#[derive(Deserialize)]
struct Ran {
  answer: Option<String>,
}

/// A gate's options as the voice reads them.
#[derive(Clone, Debug, Deserialize)]
pub struct Options {
  /// Whether the gate is switched on, as a flag or a number; none where no choice is written, which reads as on.
  pub enabled: Option<serde_json::Value>,
}

impl Options {
  /// Whether the options leave the gate on: only an explicit false or zero switches it off.
  pub fn on(&self) -> bool {
    match &self.enabled {
      Some(serde_json::Value::Bool(on)) => *on,
      Some(serde_json::Value::Number(number)) => number.as_i64() != Some(0),
      _ => true,
    }
  }
}

impl Gate {
  pub fn new(caller: Caller) -> Self {
    Self { caller }
  }

  /// One run of a gate on the stage's road with the stage's own instructions: its answer, none where the road
  /// gave none. A stop is the gate's 409 `stopped`; anything else that is not an answer is a fault.
  pub async fn run(&self, stage: &str, request: &GateRequest) -> Result<Option<String>, GateError> {
    let body = Run {
      prompt: &request.prompt,
      chat: request.chat_id,
      message: request.message_id,
      agent: request.agent.as_deref(),
      thinking: request.thinking,
      alongside: request.alongside,
    };
    let sent = self.caller.request(Method::POST, &["stages", stage, "runs"]).json(&body);

    match self.caller.json::<Ran>(sent).await {
      Ok(ran) => Ok(ran.answer),
      Err(UnitError::Refused {
        status: 409, error, ..
      }) => Err(GateError::Stopped(error)),
      Err(why) => Err(GateError::Fault(why.to_string())),
    }
  }

  /// Whether a gate is switched on: on where its options say nothing either way.
  pub async fn switched_on(&self, key: &str) -> Result<bool, UnitError> {
    let options: Options = self.caller.json(self.caller.request(Method::GET, &["options", key])).await?;

    Ok(options.on())
  }
}
