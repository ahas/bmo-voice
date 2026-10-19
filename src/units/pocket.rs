//! bmo-pocket's tool host, as its `docs/api.md` states it (「The control socket」, 「The session record」): an entry
//! for the system read from the catalogue and called under a record of the voice's own. The control socket is
//! HTTP/1.1 over a Unix socket the host's own user alone reaches, so it carries no token.

use std::path::{Path, PathBuf};

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use serde_json::{Value, json};
use tokio::net::UnixStream;

use super::UnitError;

/// The tool host, at the control socket the settings' `[units.pocket]` address names.
#[derive(Clone, Debug)]
pub struct Pocket {
  socket: PathBuf,
}

fn unreached(why: impl std::fmt::Display) -> UnitError {
  UnitError::Unreached {
    unit: "pocket",
    why: why.to_string(),
  }
}

impl Pocket {
  pub fn new(socket: impl Into<PathBuf>) -> Self {
    Self {
      socket: socket.into(),
    }
  }

  // One question to the control socket: its status and its answer read as JSON, null where it holds none.
  async fn ask(&self, method: Method, path: &str, body: Option<&Value>) -> Result<(u16, Value), UnitError> {
    let stream = UnixStream::connect(&self.socket).await.map_err(unreached)?;
    let (mut sender, connection) =
      hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.map_err(unreached)?;

    tokio::spawn(connection);

    let bytes = body.map(|body| Bytes::from(body.to_string())).unwrap_or_default();
    let request = Request::builder()
      .method(method)
      .uri(path)
      .header("host", "localhost")
      .header("content-type", "application/json")
      .body(Full::new(bytes))
      .map_err(unreached)?;
    let answer = sender.send_request(request).await.map_err(unreached)?;
    let status = answer.status().as_u16();
    let said = answer.into_body().collect().await.map_err(unreached)?.to_bytes();

    Ok((status, serde_json::from_slice(&said).unwrap_or(Value::Null)))
  }

  /// One entry for the system called by its full name, under a record of the voice's own holding the entry's own
  /// capabilities and the data root: the tool result as the host gives it.
  pub async fn call(&self, tool: &str, arguments: Value, data_root: &Path) -> Result<Value, UnitError> {
    let (status, entry) = self.ask(Method::GET, &format!("/catalogue/{tool}"), None).await?;

    if status != 200 {
      return Err(UnitError::Refused {
        unit: "pocket",
        status,
        code: None,
        error: format!("the tool host serves no {tool}"),
      });
    }

    let record = json!({
      "line": "voice",
      "capabilities": entry["capabilities"],
      "dataRoot": data_root,
      "runVariables": { "BMO_TURN_PRIVILEGED": "1" },
    });
    let (_, answer) = self
      .ask(
        Method::POST,
        "/calls",
        Some(&json!({ "session": record, "tool": tool, "arguments": arguments })),
      )
      .await?;

    Ok(answer)
  }
}
