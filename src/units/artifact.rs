//! bmo-artifact, the archive, as its `docs/api.md` states it (「Attachments」): a file attached to a document.

use base64::Engine;
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;

use super::{Caller, UnitError};

/// A file to attach to a document.
pub struct NewAttachment<'a> {
  /// The name it is shown and named by.
  pub name: &'a str,
  pub media_type: &'a str,
  /// The archive's word for its part in the document, where it has one.
  pub role: Option<&'a str>,
  pub content: &'a [u8],
}

/// A file a document keeps, as far as the voice reads it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
  pub id: i64,
  pub name: String,
}

/// The archive.
#[derive(Clone, Debug)]
pub struct Artifact {
  caller: Caller,
}

impl Artifact {
  pub fn new(caller: Caller) -> Self {
    Self { caller }
  }

  /// `POST /documents/{documentId}/attachments`: the file kept by the document, its bytes sent in base64.
  pub async fn attach(&self, document_id: i64, file: &NewAttachment<'_>) -> Result<Attachment, UnitError> {
    let request = self
      .caller
      .request(Method::POST, &["documents", &document_id.to_string(), "attachments"])
      .json(&json!({
        "name": file.name,
        "mediaType": file.media_type,
        "role": file.role,
        "content": base64::engine::general_purpose::STANDARD.encode(file.content),
      }));

    self.caller.json(request).await
  }
}
