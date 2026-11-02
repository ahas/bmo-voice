//! The service's event log (`docs/boundary-api.md`, 「Events」): each event numbered and kept in the store with the
//! member it concerns, a window of the latest kept for a reader resuming, and every live reader handed the events
//! in the order the numbers run.

use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use tokio::sync::{Mutex, MutexGuard, broadcast};
use tokio_util::sync::CancellationToken;

use crate::store::{self, StoreError};
use crate::wire::Event;

/// The kind an event of a synthesis taken in.
pub const SYNTHESIS_CREATED: &str = "synthesis.created";
/// The kind an event of a synthesis that ended, done or failed.
pub const SYNTHESIS_ENDED: &str = "synthesis.ended";
/// The kind an event of a transcription that ended.
pub const TRANSCRIPTION_ENDED: &str = "transcription.ended";
/// The kind an event of a voice whose row changed.
pub const VOICE_CHANGED: &str = "voice.changed";

/// The kind an event of a briefing taken in.
pub const BRIEFING_CREATED: &str = "briefing.created";
/// The kind an event of a briefing that ended, done or failed.
pub const BRIEFING_ENDED: &str = "briefing.ended";
/// The kind an event of a briefing whose line was named or let go.
pub const BRIEFING_CHANGED: &str = "briefing.changed";

/// Every kind the service publishes, in the order the document lists them.
pub const KINDS: [&str; 7] = [
  SYNTHESIS_CREATED,
  SYNTHESIS_ENDED,
  TRANSCRIPTION_ENDED,
  VOICE_CHANGED,
  BRIEFING_CREATED,
  BRIEFING_ENDED,
  BRIEFING_CHANGED,
];

// How many events a live reader may fall behind before it is let go, to resume from the window.
const BACKLOG: usize = 256;

/// The event log of the service.
#[derive(Clone)]
pub struct Events {
  db: sea_orm::DatabaseConnection,
  live: broadcast::Sender<Event>,
  // Held from a write's start to its event's announcement, so live readers hear the numbers in order.
  writing: Arc<Mutex<()>>,
  // Cancelled once the service is asked to stop, which ends every live reader's stream.
  stopping: CancellationToken,
}

/// A write under way: its event is numbered, and announced once it is recorded.
pub struct Writing<'a> {
  events: &'a Events,
  _held: MutexGuard<'a, ()>,
}

impl Writing<'_> {
  /// Records one event of `kind` about `data` and announces it.
  pub async fn record(&self, kind: &str, data: &impl Serialize) -> Result<Event, StoreError> {
    let data = serde_json::to_value(data).map_err(|why| StoreError(format!("an event's data: {why}")))?;
    let at = crate::now_ms();

    self.record_valued(kind, at, data).await
  }

  async fn record_valued(&self, kind: &str, at: i64, data: Value) -> Result<Event, StoreError> {
    let text = data.to_string();
    let id = store::events::record(&self.events.db, kind, at, &text).await?;
    let event = Event {
      id,
      kind: kind.to_owned(),
      at,
      data,
    };

    // No reader listening is no failure: the window keeps it for the next one.
    let _ = self.events.live.send(event.clone());

    Ok(event)
  }
}

impl Events {
  pub fn new(db: sea_orm::DatabaseConnection) -> Self {
    Self {
      db,
      live: broadcast::channel(BACKLOG).0,
      writing: Arc::new(Mutex::new(())),
      stopping: CancellationToken::new(),
    }
  }

  /// Ends every live reader's stream, as the service is asked to stop, so the stop waits on no reader.
  pub fn stop(&self) {
    self.stopping.cancel();
  }

  /// Done once the service is asked to stop.
  pub async fn stopped(&self) {
    self.stopping.cancelled().await;
  }

  /// Starts a write whose events are announced in number order.
  pub async fn write(&self) -> Writing<'_> {
    Writing {
      events: self,
      _held: self.writing.lock().await,
    }
  }

  /// A live reader's channel.
  pub fn listen(&self) -> broadcast::Receiver<Event> {
    self.live.subscribe()
  }

  /// The kept events past `after` a reader wants, at most `limit`.
  pub async fn since(
    &self,
    after: i64,
    kinds: Option<&[String]>,
    limit: Option<usize>,
  ) -> Result<Vec<Event>, StoreError> {
    store::events::since(&self.db, after, kinds, limit).await
  }

  /// The oldest event number kept, or none where none is kept.
  pub async fn first(&self) -> Result<Option<i64>, StoreError> {
    store::events::first(&self.db).await
  }
}
