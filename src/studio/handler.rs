//! Everything the server answers under `/voice-studio` (docs/script-studio.md, 「Access and service」 and 「Sign-in and
//! sessions」).

use std::collections::HashMap;
use std::io::SeekFrom;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use cookie::Cookie;
use rand::RngCore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use super::export::studio_csv;
use super::records::{RecordError, Records};
use super::review::{Fault, validate_review};
use super::subtitles::{Speech, align_review, subtitle_file, subtitle_tracks};
use crate::units::web::Web;

pub const PREFIX: &str = "/voice-studio";

/// The most of a request body the server reads.
pub const BODY_LIMIT: usize = 2_000_000;

const COOKIE: &str = "voice_studio";
const SESSION_MS: i64 = 7 * 86_400_000;
/// How long a run of failed logins from one address counts against it, and how many it may make.
pub const ATTEMPT_WINDOW_MS: i64 = 300_000;
const ATTEMPT_LIMIT: u32 = 20;

struct Attempt {
  count: u32,
  since: i64,
}

pub struct Script {
  pub records: Records,
  /// The account service a sign-in is checked against.
  pub web: Web,
  /// The generated screen, served under the prefix, read through its path on every request.
  pub static_dir: PathBuf,
  /// How a line is matched against the subtitles.
  pub speech: Speech,
  /// The address an export names as its source.
  pub source_url: Option<String>,
  attempts: Mutex<HashMap<String, Attempt>>,
  now: Box<dyn Fn() -> i64 + Send + Sync>,
}

impl Script {
  pub fn new(
    records: Records,
    web: Web,
    static_dir: PathBuf,
    speech: Speech,
    source_url: Option<String>,
    now: impl Fn() -> i64 + Send + Sync + 'static,
  ) -> Script {
    Script {
      records,
      web,
      static_dir,
      speech,
      source_url,
      attempts: Mutex::new(HashMap::new()),
      now: Box::new(now),
    }
  }
}

fn hashed(text: &str) -> String {
  format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// A JSON answer no cache keeps.
fn json_answer(status: StatusCode, body: &Value) -> Response {
  (
    status,
    [
      (header::CONTENT_TYPE, "application/json;charset=utf-8"),
      (header::CACHE_CONTROL, "no-store"),
    ],
    body.to_string(),
  )
    .into_response()
}

/// A refusal: its code, which the screen words for its reader, and an English sentence.
fn refusal(status: StatusCode, code: &str, said: &str) -> Response {
  json_answer(status, &json!({ "error": said, "code": code }))
}

/// A review that does not read, refused with its own code; a save another window got in first is a conflict.
fn review_refusal(fault: Fault) -> Response {
  let status = if fault == Fault::EditedElsewhere {
    StatusCode::CONFLICT
  } else {
    StatusCode::BAD_REQUEST
  };

  refusal(status, fault.code(), fault.sentence())
}

fn not_found() -> Response {
  (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "text/plain;charset=utf-8")], "Not found").into_response()
}

fn failed(fault: impl std::fmt::Display) -> Response {
  eprintln!("script-studio: {fault}");
  (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response()
}

fn record_failure(fault: RecordError) -> Response {
  match fault {
    RecordError::Review(fault) => review_refusal(fault),
    RecordError::Store(why) => failed(why),
  }
}

/// The paths under the prefix that belong to the server; every other GET is the screen's.
fn server_route(route: &str) -> bool {
  ["/api", "/media", "/subtitle"]
    .iter()
    .any(|own| route == *own || route.starts_with(&format!("{own}/")))
}

// The path a route names under the screen's directory, its dots walked, and never above it.
fn screen_path(root: &Path, route: &str) -> Option<PathBuf> {
  let decoded = percent_encoding::percent_decode_str(route).decode_utf8().ok()?;
  let mut path = root.to_owned();

  for part in Path::new(decoded.as_ref()).components() {
    match part {
      Component::Normal(name) => path.push(name),
      Component::ParentDir if !path.pop() || !path.starts_with(root) => return None,
      _ => {}
    }
  }

  (path != root && path.starts_with(root)).then_some(path)
}

fn content_type(path: &Path) -> String {
  let guessed = mime_guess::from_path(path).first_or_octet_stream();
  let textual = guessed.type_() == "text"
    || matches!(guessed.essence_str(), "application/json" | "application/javascript");

  if textual {
    format!("{};charset=utf-8", guessed.essence_str())
  } else {
    guessed.essence_str().to_owned()
  }
}

async fn screen(root: &Path, route: &str, head: bool) -> Response {
  let file = screen_path(root, route).filter(|path| path.is_file());
  let is_file = file.is_some();
  let target = file.unwrap_or_else(|| root.join("index.html"));

  if !target.is_file() {
    return not_found();
  }

  let cache = if is_file && route.starts_with("/_nuxt/") {
    "public, max-age=31536000, immutable"
  } else {
    "no-cache"
  };
  let body = if head {
    Body::empty()
  } else {
    match tokio::fs::read(&target).await {
      Ok(bytes) => Body::from(bytes),
      Err(_) => return not_found(),
    }
  };
  let mut response = Response::new(body);
  let headers = response.headers_mut();

  headers.insert(
    header::CONTENT_TYPE,
    HeaderValue::from_str(&content_type(&target)).expect("a media type"),
  );
  headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
  response
}

// Whether a write comes from this site: its `Origin` names this host.
fn from_this_site(headers: &HeaderMap) -> bool {
  let header = |name: header::HeaderName| headers.get(name).and_then(|value| value.to_str().ok());
  let (Some(origin), Some(host)) =
    (header(header::ORIGIN).filter(|origin| !origin.is_empty()), header(header::HOST))
  else {
    return false;
  };
  let Ok(origin) = url::Url::parse(origin) else {
    return false;
  };

  match (origin.host_str(), origin.port()) {
    (Some(name), Some(port)) => format!("{name}:{port}") == host,
    (Some(name), None) => name == host,
    (None, _) => false,
  }
}

// One file of the data root as the answer's body, from `start` for `length` bytes.
async fn file_body(path: &Path, start: u64, length: u64) -> std::io::Result<Body> {
  let mut file = tokio::fs::File::open(path).await?;

  file.seek(SeekFrom::Start(start)).await?;
  Ok(Body::from_stream(ReaderStream::new(file.take(length))))
}

/// Whether a login from this address is taken: the counts whose window has passed dropped first, so the table holds
/// only addresses seen in the last five minutes, and this attempt counted.
fn admitted(attempts: &mut HashMap<String, Attempt>, address: &str, at: i64) -> bool {
  attempts.retain(|_, entry| at - entry.since <= ATTEMPT_WINDOW_MS);

  let attempt = attempts.entry(address.to_owned()).or_insert(Attempt {
    count: 0,
    since: at,
  });

  attempt.count += 1;
  attempt.count <= ATTEMPT_LIMIT
}

async fn login(script: &Script, address: &str, headers: &HeaderMap, body: &[u8]) -> Response {
  let admitted = admitted(
    &mut script.attempts.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
    address,
    (script.now)(),
  );

  if !admitted {
    return refusal(
      StatusCode::TOO_MANY_REQUESTS,
      "sign_in_throttled",
      "Too many sign-ins from here; try again in a moment.",
    );
  }

  let bad = || refusal(StatusCode::BAD_REQUEST, "bad_sign_in", "A sign-in names a name and a password.");
  let Ok(Value::Object(sent)) = serde_json::from_slice::<Value>(body) else {
    return bad();
  };
  let (Some(name), Some(password)) =
    (sent.get("name").and_then(Value::as_str), sent.get("password").and_then(Value::as_str))
  else {
    return bad();
  };

  // Lengths as the screen counts them, in UTF-16 code units.
  if name.encode_utf16().count() > 100 || password.encode_utf16().count() > 1000 {
    return bad();
  }

  match script.web.signs_in(name, password).await {
    Ok(true) => {}
    Ok(false) => {
      return refusal(
        StatusCode::UNAUTHORIZED,
        "wrong_sign_in",
        "The name or the password does not match.",
      );
    }
    Err(why) => {
      eprintln!("script-studio: {why}");
      return refusal(
        StatusCode::SERVICE_UNAVAILABLE,
        "sign_in_unavailable",
        "The account service did not answer.",
      );
    }
  }

  let mut bytes = [0u8; 32];

  rand::rng().fill_bytes(&mut bytes);

  let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
  let now = (script.now)();

  if let Err(why) = script.records.open_session(&hashed(&token), name, now, now + SESSION_MS).await {
    return failed(why);
  }

  script
    .attempts
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner)
    .remove(address);

  let secure = if headers.get("x-forwarded-proto").is_some_and(|proto| proto == "https") {
    "; Secure"
  } else {
    ""
  };
  let cookie =
    format!("{COOKIE}={token}; Path={PREFIX}; HttpOnly; SameSite=Strict; Max-Age=604800{secure}");
  let mut response = json_answer(StatusCode::OK, &json!({ "name": name }));

  response
    .headers_mut()
    .insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).expect("a cookie"));
  response
}

fn attachment(content_type: &'static str, filename: &str, body: String) -> Response {
  (
    [
      (header::CONTENT_TYPE, content_type.to_owned()),
      (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{filename}\"")),
      (header::CACHE_CONTROL, "no-store".to_owned()),
    ],
    body,
  )
    .into_response()
}

fn unsupported() -> Response {
  refusal(StatusCode::METHOD_NOT_ALLOWED, "unsupported", "This request is not supported.")
}

async fn answer(
  State(script): State<Arc<Script>>,
  ConnectInfo(peer): ConnectInfo<SocketAddr>,
  request: Request,
) -> Response {
  let (parts, body) = request.into_parts();
  // The path as a browser reads it, dot segments and their encoded forms walked.
  let read =
    url::Url::parse(&format!("http://localhost{}", parts.uri.path())).map(|url| url.path().to_owned());
  let path = read.as_deref().unwrap_or(parts.uri.path());

  if path != PREFIX && !path.starts_with(&format!("{PREFIX}/")) {
    return not_found();
  }

  let route = &path[PREFIX.len()..];
  let query = parts
    .uri
    .query()
    .filter(|query| !query.is_empty())
    .map(|query| format!("?{query}"))
    .unwrap_or_default();
  let method = parts.method.clone();
  let headers = parts.headers;
  let head = method == Method::HEAD;
  // Nothing larger than the server takes is read at all.
  let declared = headers
    .get(header::CONTENT_LENGTH)
    .and_then(|value| value.to_str().ok()?.parse::<usize>().ok());

  if declared.is_some_and(|length| length > BODY_LIMIT) {
    return StatusCode::PAYLOAD_TOO_LARGE.into_response();
  }

  if method != Method::GET && !head && !from_this_site(&headers) {
    return refusal(StatusCode::FORBIDDEN, "foreign_origin", "The request's origin is not this site.");
  }

  if route.is_empty() {
    return (
      StatusCode::PERMANENT_REDIRECT,
      [
        (header::LOCATION, format!("{PREFIX}/{query}")),
        (header::CACHE_CONTROL, "no-store".into()),
      ],
    )
      .into_response();
  }

  if (method == Method::GET || head) && !server_route(route) {
    return screen(&script.static_dir, route, head).await;
  }

  let Ok(body) = axum::body::to_bytes(body, BODY_LIMIT).await else {
    return StatusCode::PAYLOAD_TOO_LARGE.into_response();
  };
  let token = headers
    .get(header::COOKIE)
    .and_then(|value| value.to_str().ok())
    .and_then(|cookies| {
      Cookie::split_parse(cookies)
        .flatten()
        .find(|cookie| cookie.name() == COOKIE)
        .map(|cookie| cookie.value().to_owned())
    })
    .unwrap_or_default();
  let session = script.records.session_of(&hashed(&token), (script.now)()).await;

  if route == "/api/login" && method == Method::POST {
    return login(&script, &peer.ip().to_string(), &headers, &body).await;
  }

  let Some(name) = session else {
    return refusal(StatusCode::UNAUTHORIZED, "signed_out", "Sign in first.");
  };
  let root = &script.records.root;

  if route == "/api/logout" && method == Method::POST {
    script.records.close_session(&hashed(&token)).await;

    return (
      StatusCode::NO_CONTENT,
      [(
        header::SET_COOKIE,
        format!("{COOKIE}=; Path={PREFIX}; HttpOnly; SameSite=Strict; Max-Age=0"),
      )],
    )
      .into_response();
  }

  if route == "/api/assets" && method == Method::GET {
    return match script.records.list().await {
      Ok(listed) => json_answer(StatusCode::OK, &json!({ "name": name, "assets": listed })),
      Err(fault) => record_failure(fault),
    };
  }

  if (route == "/api/export" || route == "/api/export.csv" || route == "/api/template.csv")
    && method == Method::GET
  {
    let everything = match root.assets() {
      Ok(everything) => everything,
      Err(fault) => return failed(fault),
    };
    let template = route == "/api/template.csv";
    let mut episodes = Vec::new();

    for asset in if template { Vec::new() } else { everything } {
      match script.records.read(&asset.id).await {
        Ok(review) => episodes.push((asset, review)),
        Err(fault) => return record_failure(fault),
      }
    }

    if route == "/api/export" {
      let listed: Vec<Value> = episodes
        .iter()
        .map(|(asset, review)| {
          let mut entry = asset.fields();

          entry.insert("review".into(), review.clone());
          Value::Object(entry)
        })
        .collect();
      let exported_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
      let document = json!({ "exportedAt": exported_at, "source": script.source_url, "episodes": listed });
      let text = serde_json::to_string_pretty(&document).expect("an export is plain data");

      return attachment("application/json; charset=utf-8", "voice-studio.json", text);
    }

    let filename = if template { "metadata-template.csv" } else { "bmo-voice-metadata.csv" };

    return match studio_csv(&episodes) {
      Ok(text) => attachment("text/csv; charset=utf-8", filename, text),
      Err(fault) => failed(fault),
    };
  }

  let Some((kind, id)) =
    route.strip_prefix('/').and_then(|rest| rest.rsplit_once('/')).filter(|(kind, id)| {
      ["api/episode", "media", "subtitle"].contains(kind)
        && id.len() == 64
        && id.chars().all(|one| matches!(one, '0'..='9' | 'a'..='f'))
    })
  else {
    return refusal(StatusCode::NOT_FOUND, "not_found", "Not found.");
  };
  let asset = match root.assets() {
    Ok(everything) => everything.into_iter().find(|asset| asset.id == id),
    Err(fault) => return failed(fault),
  };
  let Some(asset) = asset else {
    return refusal(StatusCode::NOT_FOUND, "no_file", "The material holds no such file.");
  };

  if kind == "api/episode" {
    if method == Method::GET {
      return match script.records.read(&asset.id).await {
        Ok(review) => {
          let mut entry = asset.fields();

          entry.insert("review".into(), review);
          entry.insert("subtitleTracks".into(), Value::Array(subtitle_tracks(&root.path, &asset.id)));
          json_answer(StatusCode::OK, &Value::Object(entry))
        }
        Err(fault) => record_failure(fault),
      };
    }

    if method == Method::PUT {
      let Ok(sent) = serde_json::from_slice::<Value>(&body) else {
        return review_refusal(Fault::RecordForm);
      };
      let review = match validate_review(&sent) {
        Ok(review) => review,
        Err(fault) => return review_refusal(fault),
      };
      let aligned = align_review(&script.speech, &root.path, &asset.id, &review);

      return match script.records.write(&asset.id, &aligned).await {
        Ok(written) => json_answer(StatusCode::OK, &written),
        Err(fault) => record_failure(fault),
      };
    }

    return unsupported();
  }

  if method != Method::GET && !head {
    return unsupported();
  }

  if kind == "subtitle" {
    let asked = url::form_urlencoded::parse(query.trim_start_matches('?').as_bytes())
      .find(|(key, _)| key == "track")
      .map(|(_, value)| value.into_owned());
    // The track's number as the screen writes it; none asked is the first.
    let index = match asked.as_deref().map(str::trim) {
      None | Some("") => Some(0.0),
      Some(written) => written.parse::<f64>().ok(),
    };
    let Some(index) =
      index.filter(|index| index.fract() == 0.0 && *index >= 0.0 && *index <= 9_007_199_254_740_991.0)
    else {
      return refusal(
        StatusCode::BAD_REQUEST,
        "bad_track",
        "A subtitle track is a whole number from 0 up.",
      );
    };
    let index = index as u64;
    let no_subtitle =
      || refusal(StatusCode::NOT_FOUND, "no_subtitle", "The video has no such subtitle track.");

    if !subtitle_tracks(&root.path, &asset.id)
      .iter()
      .any(|track| track["index"].as_u64() == Some(index))
    {
      return no_subtitle();
    }

    let Some(file) = subtitle_file(&root.path, &asset.id, index) else {
      return no_subtitle();
    };
    let body = if head {
      Body::empty()
    } else {
      Body::from(tokio::fs::read(&file).await.unwrap_or_default())
    };

    return ([(header::CONTENT_TYPE, "text/vtt; charset=utf-8")], body).into_response();
  }

  if root.download_of(&asset).and_then(|download| download.get("status").cloned())
    != Some(json!("complete"))
  {
    return refusal(StatusCode::CONFLICT, "not_downloaded", "The file's download has not finished.");
  }

  let file = root.path.join("media").join(&asset.path);
  let Ok(size) = tokio::fs::metadata(&file).await.map(|meta| meta.len()) else {
    return refusal(StatusCode::NOT_FOUND, "no_local_file", "The downloaded file is not on this machine.");
  };

  if !asset.video {
    let body = if head {
      Body::empty()
    } else {
      file_body(&file, 0, size).await.unwrap_or_else(|_| Body::empty())
    };

    return (
      [
        (header::CONTENT_TYPE, "application/octet-stream"),
        (header::CONTENT_DISPOSITION, "attachment"),
      ],
      body,
    )
      .into_response();
  }

  let mut response_headers = HeaderMap::new();

  response_headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
  response_headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
  response_headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=3600"));

  let range = headers
    .get(header::RANGE)
    .and_then(|value| value.to_str().ok())
    .filter(|range| !range.is_empty());
  let (status, start, length) = match range {
    None => (StatusCode::OK, 0, size),
    Some(range) => {
      let ranges = http_range_header::parse_range_header(range).and_then(|parsed| parsed.validate(size));

      match ranges.as_deref() {
        Ok([one]) => {
          let (start, end) = (*one.start(), *one.end());

          response_headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{size}")).expect("a range"),
          );
          (StatusCode::PARTIAL_CONTENT, start, end - start + 1)
        }
        _ => {
          return (StatusCode::RANGE_NOT_SATISFIABLE, [(header::CONTENT_RANGE, format!("bytes */{size}"))])
            .into_response();
        }
      }
    }
  };

  response_headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));

  let body = if head {
    Body::empty()
  } else {
    file_body(&file, start, length).await.unwrap_or_else(|_| Body::empty())
  };
  let mut response = Response::new(body);

  *response.status_mut() = status;
  *response.headers_mut() = response_headers;
  response
}

pub fn router(script: Arc<Script>) -> Router {
  Router::new().fallback(answer).with_state(script)
}
