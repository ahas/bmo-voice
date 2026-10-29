//! The voice's own client (`docs/boundary-api.md`, 「The client」): one method for each route, sending and reading
//! the bodies of [`crate::wire`], and a failure read back as its status, code and sentence. It carries the
//! contract and no caller's policy.

use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, RANGE};
use reqwest::{Method, RequestBuilder, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::wire::{
  Briefing, Encoding, Event, EventQuery, Health, NewBriefing, NewEncoding, NewScriptReading, NewSynthesis,
  NewTranscription, Page, PageQuery, ScriptReading, Synthesis, Transcription, Voice, VoiceServer,
  WireError,
};

/// The query of a route that takes none.
const NO_QUERY: [(&str, &str); 0] = [];

/// How long one call waits for its answer.
const PATIENCE: Duration = Duration::from_secs(60);

/// Why a call came to no answer of the route's shape.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ClientFailure {
  /// The service answered with a status that is no success: its error body where it was one, and the body as it
  /// came.
  #[error("the voice service answered {status}: {}", answer.as_ref().map_or(body.as_str(), |answer| answer.error.as_str()))]
  Answered {
    status: u16,
    answer: Option<WireError>,
    body: String,
  },
  /// Nothing answered at the address.
  #[error("the voice service could not be reached: {0}")]
  Unreached(String),
  /// A success came back in no shape the route answers.
  #[error("the voice service's answer does not read: {0}")]
  Unreadable(String),
}

type Called<T> = Result<T, ClientFailure>;

/// The client of one voice service: its address and its bearer token.
#[derive(Clone, Debug)]
pub struct VoiceClient {
  base: Url,
  token: Option<String>,
  http: reqwest::Client,
}

impl VoiceClient {
  /// A client of the service whose routes stand at `base`, sending `token` as its bearer token where one is
  /// given.
  pub fn new(base: &str, token: Option<String>) -> Called<Self> {
    let base = Url::parse(base).map_err(|why| ClientFailure::Unreached(format!("{base}: {why}")))?;
    let http = reqwest::Client::builder()
      .timeout(PATIENCE)
      .build()
      .map_err(|why| ClientFailure::Unreached(why.to_string()))?;

    Ok(Self {
      base,
      token: token.filter(|token| !token.is_empty()),
      http,
    })
  }

  // The address of a route's path, each segment given encoded as a path segment.
  fn address(&self, segments: &[&str]) -> Url {
    let mut address = self.base.clone();

    if let Ok(mut path) = address.path_segments_mut() {
      path.pop_if_empty().extend(segments);
    }

    address
  }

  fn request(&self, method: Method, segments: &[&str]) -> RequestBuilder {
    let request = self.http.request(method, self.address(segments)).header(ACCEPT, "application/json");

    match &self.token {
      Some(token) => request.header(AUTHORIZATION, format!("Bearer {token}")),
      None => request,
    }
  }

  // One call sent; its answer read as the route's shape.
  async fn read<T: DeserializeOwned>(request: RequestBuilder) -> Called<T> {
    let answered = request.send().await.map_err(|why| ClientFailure::Unreached(why.to_string()))?;
    let status = answered.status().as_u16();
    let body = answered.text().await.map_err(|why| ClientFailure::Unreadable(why.to_string()))?;

    if !(200..300).contains(&status) {
      return Err(ClientFailure::Answered {
        status,
        answer: serde_json::from_str(&body).ok(),
        body,
      });
    }

    serde_json::from_str(&body).map_err(|why| ClientFailure::Unreadable(why.to_string()))
  }

  async fn get<T: DeserializeOwned>(&self, segments: &[&str], query: &impl Serialize) -> Called<T> {
    Self::read(self.request(Method::GET, segments).query(query)).await
  }

  async fn send<T: DeserializeOwned>(
    &self,
    method: Method,
    segments: &[&str],
    body: &impl Serialize,
  ) -> Called<T> {
    Self::read(self.request(method, segments).json(body)).await
  }

  /// `GET /health`.
  pub async fn health(&self) -> Called<Health> {
    self.get(&["health"], &NO_QUERY).await
  }

  /// `GET /events`, as a page.
  pub async fn events(&self, query: &EventQuery) -> Called<Page<Event>> {
    self.get(&["events"], query).await
  }

  /// `GET /voices`.
  pub async fn voices(&self, query: &PageQuery) -> Called<Page<Voice>> {
    self.get(&["voices"], query).await
  }

  /// `GET /voices/{voiceName}`.
  pub async fn voice(&self, voice_name: &str) -> Called<Voice> {
    self.get(&["voices", voice_name], &NO_QUERY).await
  }

  /// `POST /syntheses`.
  pub async fn create_synthesis(&self, asked: &NewSynthesis) -> Called<Synthesis> {
    self.send(Method::POST, &["syntheses"], asked).await
  }

  /// `GET /syntheses/{synthesisId}`.
  pub async fn synthesis(&self, synthesis_id: i64) -> Called<Synthesis> {
    self.get(&["syntheses", &synthesis_id.to_string()], &NO_QUERY).await
  }

  /// `GET /syntheses/{synthesisId}/sound`: the sound's bytes, from `range` on where one is given.
  pub async fn synthesis_sound(&self, synthesis_id: i64, range: Option<&str>) -> Called<Vec<u8>> {
    let mut request = self.request(Method::GET, &["syntheses", &synthesis_id.to_string(), "sound"]);

    if let Some(range) = range {
      request = request.header(RANGE, range);
    }

    let answered = request.send().await.map_err(|why| ClientFailure::Unreached(why.to_string()))?;
    let status = answered.status().as_u16();

    if !(200..300).contains(&status) {
      let body = answered.text().await.unwrap_or_default();

      return Err(ClientFailure::Answered {
        status,
        answer: serde_json::from_str(&body).ok(),
        body,
      });
    }

    answered
      .bytes()
      .await
      .map(|bytes| bytes.to_vec())
      .map_err(|why| ClientFailure::Unreadable(why.to_string()))
  }

  /// `POST /transcriptions`.
  pub async fn create_transcription(&self, asked: &NewTranscription) -> Called<Transcription> {
    self.send(Method::POST, &["transcriptions"], asked).await
  }

  /// `GET /transcriptions/{transcriptionId}`.
  pub async fn transcription(&self, transcription_id: i64) -> Called<Transcription> {
    self.get(&["transcriptions", &transcription_id.to_string()], &NO_QUERY).await
  }

  /// `POST /briefings`.
  pub async fn create_briefing(&self, asked: &NewBriefing) -> Called<Briefing> {
    self.send(Method::POST, &["briefings"], asked).await
  }

  /// `GET /briefings/{briefingId}`.
  pub async fn briefing(&self, briefing_id: i64) -> Called<Briefing> {
    self.get(&["briefings", &briefing_id.to_string()], &NO_QUERY).await
  }

  /// `POST /encodings`.
  pub async fn create_encoding(&self, asked: &NewEncoding) -> Called<Encoding> {
    self.send(Method::POST, &["encodings"], asked).await
  }

  /// `POST /script-readings`.
  pub async fn read_script(&self, asked: &NewScriptReading) -> Called<ScriptReading> {
    self.send(Method::POST, &["script-readings"], asked).await
  }

  /// `GET /voice-servers`.
  pub async fn voice_servers(&self, query: &PageQuery) -> Called<Page<VoiceServer>> {
    self.get(&["voice-servers"], query).await
  }
}
