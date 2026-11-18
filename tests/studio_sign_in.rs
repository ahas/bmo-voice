//! The script studio's sign-in (`docs/script-studio.md`, 「Sign-in and sessions」), asked of a stand-in account
//! service that reads its sign-in body as bmo-web does (`docs/boundary-api.md`, 「bmo-web」): a name and a password
//! it takes open the studio's own session and let the account service's go, and a wrong one is refused.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use bmo_voice::settings::Settings;
use bmo_voice::studio::handler::{PREFIX, Script, router};
use bmo_voice::studio::records::{Records, Root};
use bmo_voice::studio::subtitles::Speech;
use bmo_voice::units::Caller;
use bmo_voice::units::web::Web;
use serde::Deserialize;
use serde_json::{Value, json};

static MADE: AtomicU32 = AtomicU32::new(0);

const NAME: &str = "tester";
const PASSWORD: &str = "right-password";
const ACCOUNT_SESSION: &str = "account-session-1";

/// A sign-in as the account service reads it; a body without one of its fields is refused 422.
#[derive(Deserialize)]
struct SignIn {
  username: String,
  password: String,
}

/// The account sessions the stand-in was asked to let go, by the cookie each came with.
#[derive(Default)]
struct Accounts {
  ended: Mutex<Vec<String>>,
}

async fn sign_in(Json(sent): Json<SignIn>) -> Response {
  if sent.username != NAME || sent.password != PASSWORD {
    return (
      StatusCode::UNAUTHORIZED,
      Json(json!({ "code": "unauthorized", "error": "Wrong sign-in." })),
    )
      .into_response();
  }

  (
    [(header::SET_COOKIE, format!("bmo.sid={ACCOUNT_SESSION}; Path=/; HttpOnly"))],
    Json(json!({ "username": NAME })),
  )
    .into_response()
}

async fn sign_out(State(accounts): State<Arc<Accounts>>, headers: HeaderMap) -> StatusCode {
  let cookie = headers.get(header::COOKIE).and_then(|value| value.to_str().ok()).unwrap_or_default();

  accounts.ended.lock().expect("the ledger").push(cookie.to_owned());
  StatusCode::NO_CONTENT
}

async fn listen(router: Router) -> SocketAddr {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a port");
  let at = listener.local_addr().expect("its address");

  tokio::spawn(async move {
    axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await
  });

  at
}

/// A data root of this test's own, with settings naming the stand-in account service.
struct DataRoot(std::path::PathBuf);

impl Drop for DataRoot {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.0);
  }
}

async fn studio(web_at: SocketAddr) -> (DataRoot, SocketAddr) {
  let made = MADE.fetch_add(1, Ordering::SeqCst);
  let root = std::env::temp_dir().join(format!("bmo_voice_sign_in_test_{}_{made}", std::process::id()));

  std::fs::create_dir_all(root.join("env")).expect("the data root is made");
  std::fs::create_dir_all(root.join("studio")).expect("the studio's data root is made");

  let path = root.join("env").join("voice.toml");

  std::fs::write(
    &path,
    format!(
      r#"listen = "127.0.0.1:0"
address = "http://127.0.0.1:0"
token_file = "voice.token"
database = "postgres:///unused"
data_root = "{}"

[units.node]
address = "http://[::1]:9"

[units.arbiter]
address = "http://[::1]:9"

[units.web]
address = "http://{web_at}"
"#,
      root.display()
    ),
  )
  .expect("the settings are written");

  let settings = Settings::read(&path).expect("the settings read");
  let caller =
    Caller::new("web", &settings, &settings.units.web, reqwest::Client::new()).expect("a caller");
  let records = Records::open(Root::new(&root.join("studio"), None))
    .await
    .expect("the review database opens");
  let script = Script::new(records, Web::new(caller), root.join("screen"), Speech::new(&[]), None, || 0);
  let at = listen(router(Arc::new(script))).await;

  (DataRoot(root), at)
}

async fn log_in(at: SocketAddr, password: &str) -> reqwest::Response {
  reqwest::Client::new()
    .post(format!("http://{at}{PREFIX}/api/login"))
    .header(header::ORIGIN, format!("http://{at}"))
    .json(&json!({ "name": NAME, "password": password }))
    .send()
    .await
    .expect("the studio answers")
}

#[tokio::test]
async fn a_sign_in_the_account_service_takes_opens_a_studio_session_and_a_wrong_one_is_refused() {
  let accounts = Arc::new(Accounts::default());
  let web_at = listen(
    Router::new()
      .route("/sumida/auth/session", post(sign_in).delete(sign_out))
      .with_state(Arc::clone(&accounts)),
  )
  .await;
  let (_root, at) = studio(web_at).await;

  let taken = log_in(at, PASSWORD).await;

  assert_eq!(taken.status(), StatusCode::OK);
  let session = taken
    .headers()
    .get(header::SET_COOKIE)
    .and_then(|value| value.to_str().ok())
    .unwrap_or_default()
    .to_owned();
  assert!(session.starts_with("bmo_script="), "{session}");
  assert_eq!(taken.json::<Value>().await.expect("a body"), json!({ "name": NAME }));
  assert_eq!(
    *accounts.ended.lock().expect("the ledger"),
    [format!("bmo.sid={ACCOUNT_SESSION}")],
    "the account service's session is let go at once"
  );

  let refused = log_in(at, "wrong-password").await;

  assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
  assert_eq!(refused.json::<Value>().await.expect("a body")["code"], "wrong_sign_in");
}
