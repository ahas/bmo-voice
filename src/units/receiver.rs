//! bmo-receiver, as its `docs/boundary-api.md` states it: a notice the room's voice says.

/// A notice for a room's voice to say in its own words (the receiver's `POST /rooms/{chat}/notices`): the fact,
/// what the line is for and what follows it, whether it may go unsaid, the kind a newer notice replaces, how long
/// it may wait, the events its delivery carries, and how long it rides the next turn rather than opening one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notice {
  pub fact: String,
  pub intent: String,
  pub follow_up: Option<String>,
  pub optional: bool,
  pub kind: Option<String>,
  pub ttl_ms: Option<i64>,
  pub events: Vec<String>,
  pub rides_ms: Option<i64>,
}
