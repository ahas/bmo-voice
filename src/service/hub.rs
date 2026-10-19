//! The listener stream's frames (`docs/boundary-api.md`, 「The listener stream」): each filed with an id, kept in a
//! ring a reconnecting listener resumes from, and handed to every listener holding the stream open.

use std::collections::VecDeque;
use std::sync::Mutex;

use tokio::sync::broadcast;

// How many frames are kept for a listener that reconnects.
const KEPT: usize = 200;

/// One frame of the stream: its id, its event name and its JSON data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
  pub id: u64,
  pub event: &'static str,
  pub data: String,
}

/// The stream's frames: the ring kept for resuming, and the channel every open stream listens on.
pub struct Hub {
  kept: Mutex<(u64, VecDeque<Frame>)>,
  sender: broadcast::Sender<Frame>,
}

impl Default for Hub {
  fn default() -> Self {
    Self {
      kept: Mutex::new((0, VecDeque::new())),
      sender: broadcast::channel(KEPT).0,
    }
  }
}

impl Hub {
  /// A frame filed: given the next id, kept, and handed to every open stream.
  pub fn file(&self, event: &'static str, data: String) {
    let frame = {
      let mut kept = self.kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

      kept.0 += 1;

      let frame = Frame {
        id: kept.0,
        event,
        data,
      };

      kept.1.push_back(frame.clone());

      while kept.1.len() > KEPT {
        kept.1.pop_front();
      }

      frame
    };

    let _ = self.sender.send(frame);
  }

  /// The frames kept after a cursor, and a receiver of every frame filed from now on. Taken together, so nothing
  /// filed between the two is lost.
  pub fn after(&self, cursor: Option<u64>) -> (Vec<Frame>, broadcast::Receiver<Frame>) {
    let kept = self.kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let receiver = self.sender.subscribe();
    let replayed = match cursor {
      Some(cursor) => kept.1.iter().filter(|frame| frame.id > cursor).cloned().collect(),
      None => vec![],
    };

    (replayed, receiver)
  }
}
