//! bmo-web, as its `docs/boundary.md` states it (「sumida's own routes」): a name and a password checked by signing in
//! with them, and the session the sign-in answered let go at once.

use reqwest::Method;
use reqwest::header::{COOKIE, SET_COOKIE};
use serde_json::json;

use super::{Caller, UnitError};

/// The cookie the web unit's session is carried by.
const SESSION_COOKIE: &str = "bmo.sid";

/// The web unit.
#[derive(Clone, Debug)]
pub struct Web {
  caller: Caller,
}

impl Web {
  pub fn new(caller: Caller) -> Self {
    Self { caller }
  }

  /// Whether the account service takes a name and a password: a sign-in it answers is a yes, and the session it
  /// opened is signed out again; a 401 is a no.
  pub async fn signs_in(&self, name: &str, password: &str) -> Result<bool, UnitError> {
    let request = self
      .caller
      .request(Method::POST, &["sumida", "auth", "session"])
      .json(&json!({ "name": name, "password": password }));
    let answer = match self.caller.send(request).await {
      Ok(answer) => answer,
      Err(UnitError::Refused { status: 401, .. }) => return Ok(false),
      Err(why) => return Err(why),
    };
    let session = answer
      .headers()
      .get_all(SET_COOKIE)
      .iter()
      .filter_map(|value| value.to_str().ok())
      .filter_map(|value| cookie::Cookie::parse(value.to_owned()).ok())
      .find(|cookie| cookie.name() == SESSION_COOKIE);

    if let Some(session) = session {
      let request = self
        .caller
        .request(Method::DELETE, &["sumida", "auth", "session"])
        .header(COOKIE, format!("{SESSION_COOKIE}={}", session.value()));

      // A session that would not go ends on its own; the sign-in stands either way.
      let _ = self.caller.send(request).await;
    }

    Ok(true)
  }
}
