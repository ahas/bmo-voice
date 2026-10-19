//! bmo-artifact, as its `docs/api.md` states it (「Documents (HTTP)」): a document read for its title.

use reqwest::Method;
use serde::Deserialize;

use super::{Caller, UnitError};

/// The artifact unit.
#[derive(Clone, Debug)]
pub struct Artifact {
  caller: Caller,
}

#[derive(Deserialize)]
struct Document {
  search: Search,
}

#[derive(Deserialize)]
struct Search {
  title: Option<String>,
}

impl Artifact {
  pub fn new(caller: Caller) -> Self {
    Self { caller }
  }

  /// A document's title, where it has one; none for a document the unit does not keep.
  pub async fn document_title(&self, document: i64) -> Result<Option<String>, UnitError> {
    let request = self.caller.request(Method::GET, &["api", "archive", &document.to_string()]);

    match self.caller.json::<Document>(request).await {
      Ok(found) => Ok(found.search.title),
      Err(UnitError::Refused { status: 404, .. }) => Ok(None),
      Err(why) => Err(why),
    }
  }
}
