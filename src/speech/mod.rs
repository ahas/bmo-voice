//! Where a line becomes sound (`docs/voice-plan.md`): this machine's card behind its one line, the voice servers
//! on the voice nodes reached through each node's own shell, and the clock a speaking run is held to.

pub mod clock;
pub mod node;
pub mod synth;

/// A path or a word as one word of a POSIX shell line: as it stands where it holds nothing a shell reads, quoted
/// otherwise.
pub fn shell_word(word: &str) -> String {
  shlex::try_quote(word).map_or_else(|_| "''".to_owned(), |quoted| quoted.into_owned())
}
