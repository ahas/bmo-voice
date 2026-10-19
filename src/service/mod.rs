//! The voice service (`docs/boundary-api.md`, 「What the voice serves」): the routes other units call, the listener
//! stream, and the reader of the receiver's stream the presence is worked out from.

pub mod hub;
pub mod presence;
pub mod routes;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::json;

use crate::context::Context;
use hub::Hub;

/// What every route stands on: the voice's context, the listener stream's frames, and the token callers present.
#[derive(Clone)]
pub struct Served {
  pub context: Arc<Context>,
  pub hub: Arc<Hub>,
  pub token: Option<Arc<String>>,
}

/// A refusal: its status, its code and its English sentence (`docs/boundary-api.md`, 「Codes」).
#[derive(Debug)]
pub struct Refused {
  pub status: StatusCode,
  pub code: &'static str,
  pub error: String,
}

impl Refused {
  pub fn new(status: StatusCode, code: &'static str, error: impl Into<String>) -> Self {
    Self {
      status,
      code,
      error: error.into(),
    }
  }

  /// A failure of the service itself or of a unit it needed.
  pub fn failed(why: impl std::fmt::Display) -> Self {
    Self::new(StatusCode::INTERNAL_SERVER_ERROR, "failed", why.to_string())
  }
}

impl IntoResponse for Refused {
  fn into_response(self) -> Response {
    (self.status, axum::Json(json!({ "error": self.error, "code": self.code }))).into_response()
  }
}

// Every route but the health wants the service's token as a bearer.
async fn authorised(
  State(served): State<Served>,
  headers: HeaderMap,
  request: Request,
  next: Next,
) -> Response {
  let Some(token) = &served.token else {
    return next.run(request).await;
  };
  let presented = headers
    .get(header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "));

  if presented != Some(token.as_str()) {
    return Refused::new(StatusCode::UNAUTHORIZED, "unauthorised", "a voice route wants the voice's token")
      .into_response();
  }

  next.run(request).await
}

/// The service's routes.
pub fn router(served: Served) -> Router {
  let guarded = Router::new()
    .route("/briefings", post(routes::launch).get(routes::briefings))
    .route("/briefings/{run}/line", post(routes::line))
    .route("/briefings/{run}/sound", get(routes::sound))
    .route("/briefings/stop", post(routes::stop))
    .route("/briefings/heard", post(routes::heard))
    .route("/briefings/events", get(routes::events))
    .route("/scripts/reading", post(routes::script_reading))
    .route("/policy", get(routes::policy))
    .route_layer(middleware::from_fn_with_state(served.clone(), authorised));

  Router::new().route("/health", get(routes::health)).merge(guarded).with_state(served)
}

/// The service run: its settings read, its store migrated, the receiver's stream followed, and the routes served
/// where the settings say it listens, until it is stopped.
pub async fn serve() -> Result<(), String> {
  let context = Context::open(8).await?;
  let token = context.settings.token(&context.settings.token_file).map_err(|why| why.to_string())?;
  let hub = Arc::new(Hub::default());
  let served = Served {
    hub: Arc::clone(&hub),
    token: token.map(Arc::new),
    context: Arc::new(context),
  };
  let listener = tokio::net::TcpListener::bind(&served.context.settings.listen)
    .await
    .map_err(|why| format!("{} could not be listened on: {why}", served.context.settings.listen))?;

  tokio::spawn(presence::follow(served.context.units.receiver.clone(), hub));
  eprintln!("voice-server: listening on {}", served.context.settings.listen);

  axum::serve(listener, router(served))
    .with_graceful_shutdown(asked_to_stop())
    .await
    .map_err(|why| format!("the service stopped: {why}"))
}

// The service's unit stopping it, or a person at its terminal.
async fn asked_to_stop() {
  use tokio::signal::unix::{SignalKind, signal};

  match signal(SignalKind::terminate()) {
    Ok(mut term) => {
      tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
      }
    }
    Err(_) => {
      let _ = tokio::signal::ctrl_c().await;
    }
  }
}
