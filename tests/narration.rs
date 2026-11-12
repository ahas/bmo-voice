//! A narration's wait on its synthesis (`docs/boundary-api.md`, 「Narrating a document」): the bound that lets it go,
//! and the lines that say where it stands. Each runs on a paused clock.

use std::collections::VecDeque;
use std::time::Duration;

use bmo_voice::client::ClientFailure;
use bmo_voice::narration::{self, NotSpoken};
use bmo_voice::wire::{Synthesis, SynthesisState};
use tokio::time::Instant;

const ID: i64 = 4705;

const ON_THE_NODE: &str = "on node-a, speaking piece 3 of 9";

fn synthesis(state: SynthesisState, waiting_on: Option<&str>) -> Synthesis {
  Synthesis {
    id: ID,
    voice_name: "narrator".to_owned(),
    text: "words".to_owned(),
    spoken: None,
    emotion: None,
    key: None,
    state,
    wait: None,
    waiting_on: waiting_on.map(str::to_owned),
    failure: None,
    duration_ms: (state == SynthesisState::Done).then_some(1_000),
    created_at: 0,
  }
}

fn speaking() -> Synthesis {
  synthesis(SynthesisState::Speaking, Some(ON_THE_NODE))
}

#[tokio::test(start_paused = true)]
async fn a_synthesis_that_does_not_end_is_let_go_at_the_bound_naming_how_it_stood() {
  let within = Duration::from_secs(60 * 60);
  let began = Instant::now();
  let mut told = Vec::new();

  let ended =
    narration::wait(speaking(), within, |_| async { Ok(speaking()) }, |line| told.push(line.to_owned()))
      .await;

  assert_eq!(began.elapsed(), within);
  let Err(why) = ended else { panic!("a synthesis still speaking was answered as done") };
  assert!(
    matches!(
      why,
      NotSpoken::OutOfTime {
        state: SynthesisState::Speaking,
        ..
      }
    ),
    "{why:?}"
  );
  let sentence = why.to_string();
  assert!(sentence.contains("synthesis 4705 was still speaking after 60m00s"), "{sentence}");
  assert!(sentence.contains(ON_THE_NODE), "{sentence}");
  // Told as it began, then every five minutes it stood unchanged, the last as the bound ran out.
  assert_eq!(told.len(), 13, "{told:#?}");
  assert_eq!(told[0], format!("synthesis 4705 speaking for 0m00s: {ON_THE_NODE}"));
  assert_eq!(told[12], format!("synthesis 4705 speaking for 60m00s: {ON_THE_NODE}"));
}

#[tokio::test(start_paused = true)]
async fn a_read_that_never_comes_back_is_let_go_at_the_bound() {
  let within = Duration::from_secs(60);
  let began = Instant::now();

  let ended = narration::wait(speaking(), within, |_| std::future::pending(), |_| {}).await;

  assert_eq!(began.elapsed(), within);
  assert!(matches!(ended, Err(NotSpoken::OutOfTime { .. })), "{ended:?}");
}

#[tokio::test(start_paused = true)]
async fn each_change_is_told_an_unanswered_read_asked_again_and_the_done_synthesis_answered() {
  let mut answers = VecDeque::from([
    Ok(synthesis(SynthesisState::Waiting, Some("every card is in use"))),
    Ok(speaking()),
    Err(ClientFailure::Unreached("connection refused".to_owned())),
    Ok(speaking()),
    Ok(synthesis(SynthesisState::Done, None)),
  ]);
  let mut told = Vec::new();

  let ended = narration::wait(
    synthesis(SynthesisState::Waiting, Some("every card is in use")),
    Duration::from_secs(60),
    move |_| std::future::ready(answers.pop_front().expect("read past the last answer")),
    |line| told.push(line.to_owned()),
  )
  .await;

  assert_eq!(ended.expect("a done synthesis is answered").state, SynthesisState::Done);
  assert_eq!(
    told,
    [
      "synthesis 4705 waiting for 0m00s: every card is in use".to_owned(),
      "synthesis 4705 speaking for 0m01s: on node-a, speaking piece 3 of 9".to_owned(),
      "synthesis 4705: the voice service did not answer, asked again: connection refused".to_owned(),
    ]
  );
}

#[tokio::test(start_paused = true)]
async fn a_failed_synthesis_ends_the_wait_with_the_engines_reason() {
  let mut failed = synthesis(SynthesisState::Failed, None);
  failed.failure = Some("node-a did not answer within 60 seconds".to_owned());

  let ended = narration::wait(failed, Duration::from_secs(60), |_| std::future::pending(), |_| {}).await;

  let sentence = ended.expect_err("a failed synthesis is no narration").to_string();
  assert_eq!(sentence, "synthesis 4705 failed: node-a did not answer within 60 seconds");
}
