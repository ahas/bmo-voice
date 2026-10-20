//! bmo-node, as its `docs/api.md` states it (「Agency's calls」, 「Registry routes」): how a node is reached by its
//! name, and the arguments of a batch SSH login to it. Bodies are camelCase, and a refusal is an answer of its own.

use reqwest::Method;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{Caller, UnitError};

/// The node service.
#[derive(Clone, Debug)]
pub struct Node {
  caller: Caller,
}

/// How a node is reached, and the services its role reserves it for, joined by `+`, or none.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Target {
  #[serde(flatten)]
  pub reach: Reach,
  pub role: Option<String>,
}

/// An ssh node by its host, port, account, key and shell, or a hub node through its hub by the hub's address, its
/// token file and the agent the node attaches as.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Reach {
  Ssh {
    host: String,
    port: u16,
    user: String,
    #[serde(rename = "keyPath")]
    key_path: Option<String>,
    shell: Option<String>,
  },
  Jake {
    #[serde(rename = "hubUrl")]
    hub_url: String,
    #[serde(rename = "tokenFile")]
    token_file: String,
    agent: String,
  },
}

#[derive(Deserialize)]
struct Refusal {
  code: String,
  message: String,
}

#[derive(Deserialize)]
struct Login {
  ssh: Option<Vec<String>>,
}

impl Node {
  pub fn new(caller: Caller) -> Self {
    Self { caller }
  }

  // An answer read as its own object, or as the refusal it is.
  async fn answer<T: DeserializeOwned>(&self, sent: reqwest::RequestBuilder) -> Result<T, UnitError> {
    let answer: Value = self.caller.json(sent).await?;

    if answer.get("answered") == Some(&Value::Bool(false)) {
      let refusal = serde_json::from_value::<Refusal>(answer["refusal"].clone()).map_err(|why| {
        UnitError::Unreached {
          unit: "node",
          why: why.to_string(),
        }
      })?;

      return Err(UnitError::Refused {
        unit: "node",
        status: 200,
        code: Some(refusal.code),
        error: refusal.message,
      });
    }

    serde_json::from_value(answer).map_err(|why| UnitError::Unreached {
      unit: "node",
      why: why.to_string(),
    })
  }

  /// How a node is reached by its name.
  pub async fn target(&self, name: String) -> Result<Target, UnitError> {
    self.answer(self.caller.request(Method::GET, &["nodes", &name, "target"])).await
  }

  /// The arguments that open a batch SSH login to a node, ahead of the command; none where it has no SSH route.
  pub async fn login(&self, name: String) -> Result<Option<Vec<String>>, UnitError> {
    let login: Login = self
      .answer(self.caller.request(Method::POST, &["nodes", "login"]).json(&json!({ "node": name })))
      .await?;

    Ok(login.ssh)
  }
}
