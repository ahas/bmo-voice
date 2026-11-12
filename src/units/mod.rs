//! The other units the voice calls, each a client of only the routes that unit's own document states
//! (`docs/boundary-api.md`, 「What the voice calls」).

pub mod agency;
pub mod arbiter;
pub mod artifact;
pub mod hub;
pub mod node;
pub mod web;

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::settings::{Settings, Unit};

/// How many idle connections the one client keeps to each unit.
const IDLE_PER_UNIT: usize = 8;

/// How often the transport probes a connection to a unit that has gone quiet.
const KEEP_ALIVE: Duration = Duration::from_secs(30);

/// Every unit the voice calls, each at the address the settings name.
#[derive(Clone, Debug)]
pub struct Units {
  /// The node service: how each voice node is reached.
  pub node: node::Node,
  /// The arbiter's master: which node's card a synthesis is spoken on.
  pub arbiter: arbiter::Arbiter,
  /// The account service the script studio signs a person in against.
  pub web: web::Web,
  /// Agency, whose co-worker pool derives a line's spoken words; none where the settings name none.
  pub agency: Option<agency::Agency>,
}

impl Units {
  /// The units the settings name, called through one HTTP client.
  pub fn new(settings: &Settings) -> Result<Self, UnitError> {
    // One client for every call, its idle connections to each unit kept to a bound. A call waits for its answer
    // however long it takes; the transport's keep-alive probes tell a unit gone without closing the connection.
    let http = reqwest::Client::builder()
      .pool_max_idle_per_host(IDLE_PER_UNIT)
      .tcp_keepalive(KEEP_ALIVE)
      .build()
      .map_err(|why| UnitError::Unreached {
        unit: "voice",
        why: format!("the HTTP client did not stand: {why}"),
      })?;
    let units = &settings.units;
    let caller = |name: &'static str, unit: &Unit| Caller::new(name, settings, unit, http.clone());

    Ok(Self {
      node: node::Node::new(caller("node", &units.node)?),
      arbiter: arbiter::Arbiter::new(caller("arbiter", &units.arbiter)?),
      web: web::Web::new(caller("web", &units.web)?),
      agency: match &units.agency {
        Some(unit) => Some(agency::Agency::new(caller("agency", unit)?)),
        None => None,
      },
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

  // The address of a path, each part one segment of it, escaped as a path segment escapes: a mark a segment may
  // hold, such as the `@` and `:` of a session's id, stands as it is, since a unit reads its segments unescaped.
  fn url(&self, path: &[&str]) -> String {
    if let Ok(mut url) = url::Url::parse(&self.address) {
      let extended = match url.path_segments_mut() {
        Ok(mut segments) => {
          segments.pop_if_empty().extend(path);
          true
        }
        Err(()) => false,
      };

      if extended {
        return url.to_string();
      }
    }

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
