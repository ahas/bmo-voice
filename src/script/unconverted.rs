//! What a line's spoken words left unconverted or misread (docs/boundary-api.md, 「The spoken words」): a letter of a
//! script the voice's language does not write, a digit, a mark an address, a path, code or markup is written with,
//! or a number of the written words they do not say. It only finds; nothing here mends a word, since the spoken words
//! are a model's to write.

use super::numbers::{Misread, misread};
use super::reading::Language;
use super::spoken::holds_korean;

/// The marks an address, a path, code or markup is written with, which no spoken word holds.
const MARKS: &[char] = &[
  '/', '\\', '`', '*', '_', '#', '|', '<', '>', '[', ']', '{', '}', '~', '=', '^', '@', '$', '%', '&', '+',
];

/// The first word of the spoken words that holds what a voice cannot say as written, and what it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unconverted {
  pub word: String,
  pub holds: &'static str,
}

impl std::fmt::Display for Unconverted {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    if self.word.is_empty() {
      return write!(f, "the spoken words hold {}", self.holds);
    }

    write!(f, "the spoken words hold the word \"{}\", which holds {}", self.word, self.holds)
  }
}

impl Language {
  /// Whether a letter belongs to a script this language's spoken words never hold.
  fn foreign_letter(self, character: char) -> bool {
    match self {
      Self::Ko => character.is_ascii_alphabetic() || ('\u{FF21}'..='\u{FF5A}').contains(&character),
      Self::En => holds_korean(character.encode_utf8(&mut [0; 4])),
    }
  }

  /// What the check calls a letter this language's spoken words never hold.
  fn foreign_letters(self) -> &'static str {
    match self {
      Self::Ko => "a Latin letter",
      Self::En => "a Hangul letter",
    }
  }
}

// What one character holds that no spoken word may, if anything.
fn held(character: char, language: Language) -> Option<&'static str> {
  if language.foreign_letter(character) {
    Some(language.foreign_letters())
  } else if character.is_numeric() {
    Some("a digit")
  } else if MARKS.contains(&character) {
    Some("a mark an address, a path, code or markup is written with")
  } else if ('\u{E000}'..='\u{F8FF}').contains(&character) {
    Some("a private mark no voice says")
  } else {
    None
  }
}

/// The first word of `spoken` that holds what the check finds, or none where every word can be said as it stands.
/// Spoken words with nothing in them fail as a whole.
pub fn unconverted(spoken: &str, language: Language) -> Option<Unconverted> {
  if spoken.trim().is_empty() {
    return Some(Unconverted {
      word: String::new(),
      holds: "nothing to say",
    });
  }

  spoken.split_whitespace().find_map(|word| {
    word.chars().find_map(|character| held(character, language)).map(|holds| Unconverted {
      word: word.to_owned(),
      holds,
    })
  })
}

/// What the check found in a line's spoken words: a word holding what no spoken word may, or a number of the written
/// words they do not say as it is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Finding {
  Unconverted(Unconverted),
  Misread(Misread),
}

impl std::fmt::Display for Finding {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Unconverted(found) => found.fmt(f),
      Self::Misread(Misread { written, said }) => write!(
        f,
        "the spoken words do not say the written \"{written}\" as it is written; its figures read {said}"
      ),
    }
  }
}

/// The first thing the check finds in `spoken`, the spoken words of the written words `written`: a word left
/// unconverted first, then a number they do not say; none where the line can be said as it stands.
pub fn found(written: &str, spoken: &str, language: Language) -> Option<Finding> {
  unconverted(spoken, language)
    .map(Finding::Unconverted)
    .or_else(|| misread(written, spoken, language).map(Finding::Misread))
}
