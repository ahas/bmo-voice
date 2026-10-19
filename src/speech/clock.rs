//! A speaking run's cap as agency's clock keeps it (agency's `docs/api.md`, 「Workers」, a run's clock): past the
//! cap the agency asks whether to keep the run, and the speaking goes on while it decides and for as long as it
//! keeps it.

use std::time::Duration;

use crate::now_ms;
use crate::units::agency::Agency;

/// The watch over one stretch of a run, from `started_at` with `bound_ms` to its cap: it ends when the stretch is
/// to be cut — no keep stands past the bound, or the clock could not be read — and waits while a keep or an open
/// ask stands.
pub async fn watch(agency: &Agency, run: &str, started_at: i64, bound_ms: i64) {
  let mut look_at = started_at + bound_ms;

  loop {
    tokio::time::sleep(Duration::from_millis((look_at - now_ms()).max(1) as u64)).await;

    let clock = match agency.clock(run).await {
      Ok(clock) => clock,
      Err(why) => {
        eprintln!("voice: the run's clock could not be read, so the run is cut at its bound: {why}");
        return;
      }
    };
    let now = now_ms();

    match clock.kept_until.filter(|until| *until > now) {
      Some(until) => look_at = until,
      None => return,
    }
  }
}
