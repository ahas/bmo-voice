//! The other units the voice calls, each a client of only the routes that unit's own document states
//! (`docs/boundary-api.md`, 「What the voice calls」).

pub mod agency;
pub mod artifact;
pub mod gate;
pub mod hub;
pub mod node;
pub mod pocket;
pub mod receiver;
pub mod web;

use serde::de::DeserializeOwned;

use crate::settings::{Settings, Unit};

/// Every unit the voice calls, each at the address the settings name.
#[derive(Clone, Debug)]
pub struct Units {
  pub gate: gate::Gate,
  pub receiver: receiver::Receiver,
  pub agency: agency::Agency,
  pub artifact: artifact::Artifact,
  pub node: node::Node,
  pub pocket: pocket::Pocket,
  pub web: web::Web,
}

impl Units {
  /// The units the settings name, called through one HTTP client.
  pub fn new(settings: &Settings) -> Result<Self, UnitError> {
    let http = reqwest::Client::new();
    let units = &settings.units;
    let caller = |name: &'static str, unit: &Unit| Caller::new(name, settings, unit, http.clone());

    Ok(Self {
      gate: gate::Gate::new(caller("gate", &units.gate)?),
      receiver: receiver::Receiver::new(caller("receiver", &units.receiver)?),
      agency: agency::Agency::new(caller("agency", &units.agency)?),
      artifact: artifact::Artifact::new(caller("artifact", &units.artifact)?),
      node: node::Node::new(caller("node", &units.node)?),
      pocket: pocket::Pocket::new(&units.pocket.address),
      web: web::Web::new(caller("web", &units.web)?),
    })
  }
}

/// Why a unit did not give what was asked: its refusal, or no answer at all.
#[derive(Debug)]
pub enum UnitError {
  /// The unit answered with a refusal: its status, its code where it gave one, and its sentence.
  Refused {
    unit: &'static str,
    status: u16,
    code: Option<String>,
    error: String,
  },
  /// The unit could not be reached or its answer did not read.
  Unreached { unit: &'static str, why: String },
}

impl std::fmt::Display for UnitError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Refused { unit, error, .. } => write!(f, "{unit} refused: {error}"),
      Self::Unreached { unit, why } => write!(f, "{unit} did not answer: {why}"),
    }
  }
}

impl std::error::Error for UnitError {}

/// One unit's base address and the token it is called with.
#[derive(Clone, Debug)]
pub struct Caller {
  unit: &'static str,
  address: String,
  token: Option<String>,
  http: reqwest::Client,
}

#[derive(serde::Deserialize)]
struct Refusal {
  error: Option<String>,
  code: Option<String>,
}

impl Caller {
  /// A caller of the unit the settings name, with its token read from its file.
  pub fn new(
    unit: &'static str,
    settings: &Settings,
    of: &Unit,
    http: reqwest::Client,
  ) -> Result<Self, UnitError> {
    let token = settings.unit_token(of).map_err(|why| UnitError::Unreached {
      unit,
      why: why.to_string(),
    })?;

    Ok(Self {
      unit,
      address: of.address.trim_end_matches('/').to_owned(),
      token,
      http,
    })
  }

  fn url(&self, path: &[&str]) -> String {
    let mut url = self.address.clone();

    for part in path {
      url.push('/');
      url.push_str(&urlencoding::encode(part));
    }

    url
  }

  fn unreached(&self, why: impl std::fmt::Display) -> UnitError {
    UnitError::Unreached {
      unit: self.unit,
      why: why.to_string(),
    }
  }

  /// A request to a path, the token added where the unit takes one.
  pub fn request(&self, method: reqwest::Method, path: &[&str]) -> reqwest::RequestBuilder {
    let request = self.http.request(method, self.url(path));

    match &self.token {
      Some(token) => request.bearer_auth(token),
      None => request,
    }
  }

  /// A request sent; its answer where it is a success, else the unit's refusal.
  pub async fn send(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response, UnitError> {
    let answer = request.send().await.map_err(|why| self.unreached(why))?;
    let status = answer.status();

    if status.is_success() {
      return Ok(answer);
    }

    let text = answer.text().await.unwrap_or_default();
    let refusal = serde_json::from_str::<Refusal>(&text).ok();
    let error = refusal
      .as_ref()
      .and_then(|refusal| refusal.error.clone())
      .unwrap_or_else(|| format!("{} answered {}", self.unit, status.as_u16()));

    Err(UnitError::Refused {
      unit: self.unit,
      status: status.as_u16(),
      code: refusal.and_then(|refusal| refusal.code),
      error,
    })
  }

  /// A request sent and its JSON answer read.
  pub async fn json<T: DeserializeOwned>(&self, request: reqwest::RequestBuilder) -> Result<T, UnitError> {
    self.send(request).await?.json::<T>().await.map_err(|why| self.unreached(why))
  }
}
