//! The engine's own reading rules (docs/voice-plan.md, 「Which language a line is said in」, 「How names are said」,
//! 「How figures are said」): the language a line's words are in, its names said by the voice name table, its figures
//! said by rule, and the spans a Korean voice has no settled reading for marked. Everything here is mechanical;
//! no model is asked.

use std::sync::LazyLock;

use bmo_text::js;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::figures::RUN;
use super::spoken::{CODE_CLOSE, CODE_OPEN, holds_korean};

/// The language a line is spoken in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
  Ko,
  En,
}

impl Language {
  /// The word that names the language.
  pub fn code(self) -> &'static str {
    match self {
      Self::Ko => "ko",
      Self::En => "en",
    }
  }

  /// The language a word names, or none.
  pub fn parse(word: &str) -> Option<Self> {
    match word {
      "ko" => Some(Self::Ko),
      "en" => Some(Self::En),
      _ => None,
    }
  }

  /// The language the written words of a line say they are in (docs/voice-plan.md, 「Which language a line is
  /// said in」): English where more of its words hold a Latin letter and no Hangul than hold a Hangul letter,
  /// Korean where as many or more hold Hangul, and none where no word holds either.
  pub fn of_line(text: &str) -> Option<Self> {
    let (korean, english) = text.split_whitespace().fold((0usize, 0usize), |(korean, english), word| {
      if holds_korean(word) {
        (korean + 1, english)
      } else if word.chars().any(is_latin_letter) {
        (korean, english + 1)
      } else {
        (korean, english)
      }
    });

    match (korean, english) {
      (0, 0) => None,
      (korean, english) if english > korean => Some(Self::En),
      _ => Some(Self::Ko),
    }
  }
}

// A letter of the Latin alphabet, in its plain or its full-width form.
fn is_latin_letter(character: char) -> bool {
  character.is_ascii_alphabetic()
    || ('\u{FF21}'..='\u{FF3A}').contains(&character)
    || ('\u{FF41}'..='\u{FF5A}').contains(&character)
}

/// One row of the voice name table: a spelling, and what a Korean and an English line say in its place,
/// empty where that language leaves it as written (docs/voice-plan.md, 「How names are said」).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct VoiceName {
  pub written: String,
  pub ko: String,
  pub en: String,
}

impl VoiceName {
  fn reading(&self, language: Language) -> &str {
    match language {
      Language::Ko => &self.ko,
      Language::En => &self.en,
    }
  }
}

/// Whether a text holds anything a Korean voice has no settled reading for: a roman letter, or a figure,
/// which is read one way as a number and another as a date, a time or a version.
pub fn needs_reading(text: &str) -> bool {
  text.chars().any(|character| character.is_ascii_alphanumeric())
}

fn spaces_only(text: &str) -> bool {
  !text.is_empty() && text.chars().all(|character| character == ' ' || character == '\t')
}

/// One span of a Korean stretch the gate is asked to read, where it stands in the stretch by byte offsets,
/// and whether it was code in the line, which the gate reads for what it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadingSpan {
  pub text: String,
  pub start: usize,
  pub end: usize,
  pub code: bool,
}

fn is_code_mark(character: char) -> bool {
  character == CODE_OPEN || character == CODE_CLOSE
}

// A formatting mark or a code mark, never the gate's to read.
fn is_clinging_mark(character: char) -> bool {
  matches!(character, '*' | '`' | '|') || is_code_mark(character)
}

/// The spans of a stretch a Korean voice has no reading for, in order: each run that is neither Hangul nor
/// white space nor a title's particle and holds something unreadable, less the sentence's own closing marks
/// and any formatting or code mark clinging to either end, with runs apart by nothing but spaces taken as
/// one span.
pub fn reading_spans(text: &str) -> Vec<ReadingSpan> {
  let mut spans: Vec<ReadingSpan> = vec![];

  for found in RUN.find_iter(text) {
    let run = found.as_str();
    let unmarked = run.trim_start_matches(is_clinging_mark);
    let lead = run.len() - unmarked.len();
    let word = unmarked
      .trim_end_matches(|character| is_clinging_mark(character) || ".,!?\u{2026}".contains(character));

    if !needs_reading(word) {
      continue;
    }

    let start = found.start() + lead;
    let end = start + word.len();
    let code = run.contains(is_code_mark);

    match spans.last_mut() {
      Some(last) if spaces_only(&text[last.end..start]) => {
        last.end = end;
        last.text = text[last.start..end].to_owned();
        last.code |= code;
      }
      _ => spans.push(ReadingSpan {
        text: word.to_owned(),
        start,
        end,
        code,
      }),
    }
  }

  spans
}

// Whether a spelling stands at `at` as a word of its own, whatever its case: nothing that is a letter or a
// figure before it, and no roman letter or figure after it, so a Korean particle may follow.
fn name_at(text: &[char], at: usize, spelling: &[char]) -> bool {
  let end = at + spelling.len();

  end <= text.len()
    && text[at..end]
      .iter()
      .zip(spelling)
      .all(|(written, spelled)| written.to_lowercase().eq(spelled.to_lowercase()))
    && text.get(end).is_none_or(|next| !next.is_ascii_alphanumeric())
}

fn letter_or_figure(character: char) -> bool {
  static LETTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}]$").expect("a letter or a figure"));

  LETTER.is_match(character.encode_utf8(&mut [0; 4]))
}

/// A stretch with every name the voice name table holds written the way that stretch's language says it
/// (docs/voice-plan.md, 「How names are said」). A name whose reading in that language is empty is left as
/// written, and so is every other word. The longest spelling is tried first, so a name that holds a shorter
/// one inside it is read whole.
pub fn say_names(text: &str, language: Language, names: &[VoiceName]) -> String {
  let said = names
    .iter()
    .filter(|name| !name.written.is_empty() && !name.reading(language).is_empty())
    .collect::<Vec<_>>();

  if said.is_empty() {
    return text.to_owned();
  }

  let mut spellings = said.iter().map(|name| js::chars(&name.written)).collect::<Vec<_>>();

  spellings.sort_by_key(|spelling| std::cmp::Reverse(js::units_of(spelling)));

  let characters = js::chars(text);
  let mut out = String::new();
  let mut at = 0;

  while at < characters.len() {
    let free = at == 0 || !letter_or_figure(characters[at - 1]);
    let spelling = free
      .then(|| spellings.iter().find(|spelling| name_at(&characters, at, spelling)))
      .flatten();

    let Some(spelling) = spelling else {
      out.push(characters[at]);
      at += 1;
      continue;
    };
    let found = js::text(&characters[at..at + spelling.len()]);
    let name = said.iter().find(|name| name.written.to_lowercase() == found.to_lowercase());

    out.push_str(name.map_or(found.as_str(), |name| name.reading(language)));
    at += spelling.len();
  }

  out
}
