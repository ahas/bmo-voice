//! The voice service (`docs/boundary-api.md`): its settings, its store, its event log, and the routes of
//! [`crate::http`] served where the settings say it listens.

use std::sync::Arc;

use crate::context::Context;
use crate::events::Events;
use crate::http::{Served, router};
use crate::speech::{card, spoken};

/// The service run: its settings read, its store migrated, the demands and seats an earlier process left given back,
/// the lines it left unspoken taken up, and its
/// routes served until it is stopped.
pub async fn serve() -> Result<(), String> {
  let context = Context::open(8).await?;
  let left = card::give_back_left(&context.units.arbiter, &context.settings.demands_folder()).await;

  if left > 0 {
    eprintln!("voice-server: gave back {left} demands an earlier process left");
  }

  if let Some(agency) = &context.units.agency {
    let left = spoken::give_back_left(agency, &context.settings.seats_folder()).await;

    if left > 0 {
      eprintln!("voice-server: gave back {left} seats an earlier process left");
    }
  }

  let token = context.settings.token(&context.settings.token_file).map_err(|why| why.to_string())?;
  let events = Events::new(context.db.clone());
  let served = Served {
    context: Arc::new(context),
    events,
    token: token.map(Arc::new),
  };

  match crate::http::take_up(Arc::clone(&served.context), served.events.clone()).await {
    Ok(0) => {}
    Ok(taken) => eprintln!("voice-server: took up {taken} lines an earlier process left unspoken"),
    Err(why) => eprintln!("voice-server: the lines an earlier process left were not read: {why}"),
  }
  let listener = tokio::net::TcpListener::bind(&served.context.settings.listen)
    .await
    .map_err(|why| format!("{} could not be listened on: {why}", served.context.settings.listen))?;

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
