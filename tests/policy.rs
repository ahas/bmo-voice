//! The run cap grows with a line's spoken words (`docs/voice-plan.md`, 「What a long line takes of the card」): the
//! policy's `run_cap_ms`, and `run_ms_per_character` for each character, Hangul and Latin alike.

use bmo_voice::policy::POLICY;

#[test]
fn the_run_cap_grows_with_every_character_of_the_spoken_words() {
  assert_eq!(POLICY.run_cap_for(""), POLICY.run_cap_ms);
  assert_eq!(POLICY.run_cap_for("가나다"), POLICY.run_cap_ms + 3 * POLICY.run_ms_per_character);
  assert_eq!(POLICY.run_cap_for("abc"), POLICY.run_cap_for("가나다"));

  // The longest spoken words a synthesis holds are given at least their speaking time at a Korean voice's pace,
  // about 7 characters a second of sound, spoken a little faster than real time.
  let longest = "가".repeat(bmo_voice::speech::spoken::MOST_SPOKEN);

  assert!(POLICY.run_cap_for(&longest) >= POLICY.run_cap_ms + 30_000 * 1_000 / 7);
}
