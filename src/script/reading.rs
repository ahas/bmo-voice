//! The engine's own reading rules (docs/voice-plan.md, 「A line is spoken as a script」, 「How names are said」,
//! 「How figures are said」): a line cut into its languages, its names said by the voice name table, its figures
//! said by rule, and the spans a Korean voice has no settled reading for marked. Everything here is mechanical;
//! no model is asked.

use std::sync::LazyLock;

use bmo_markup::js::{self, is_space};
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::figures::RUN;
use super::spoken::{CODE_CLOSE, CODE_OPEN, holds_korean};

/// The language a stretch of a line is spoken in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
  Ko,
  En,
}

impl Language {
  /// The language a word names, or none.
  pub fn parse(word: &str) -> Option<Self> {
    match word {
      "ko" => Some(Self::Ko),
      "en" => Some(Self::En),
      _ => None,
    }
  }
}

/// One row of the voice name table: a spelling, and what a Korean and an English stretch say in its place,
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

/// One stretch of a line in one language, and where it stands in the line, by byte offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceStretch {
  pub language: Language,
  pub text: String,
  pub start: usize,
  pub end: usize,
}

/// Whether a text holds anything a Korean voice has no settled reading for: a roman letter, or a figure,
/// which is read one way as a number and another as a date, a time or a version.
pub fn needs_reading(text: &str) -> bool {
  text.chars().any(|character| character.is_ascii_alphanumeric())
}

fn is_roman(text: &str) -> bool {
  text.chars().any(|character| character.is_ascii_alphabetic())
}

fn spaces_only(text: &str) -> bool {
  !text.is_empty() && text.chars().all(|character| character == ' ' || character == '\t')
}

// Where the line breaks into sentences, by byte offsets: the white space after a full stop, a question or
// exclamation mark or an ellipsis, and every run of line breaks.
fn sentence_gaps(text: &str) -> Vec<(usize, usize)> {
  let characters = text.char_indices().collect::<Vec<_>>();
  let offset = |at: usize| characters.get(at).map_or(text.len(), |(offset, _)| *offset);
  let mut gaps = vec![];
  let mut at = 0;

  while at < characters.len() {
    let character = characters[at].1;
    let closed = at > 0 && matches!(characters[at - 1].1, '.' | '?' | '!' | '\u{2026}');
    let gap: Option<fn(char) -> bool> = if closed && is_space(character) {
      Some(is_space)
    } else if character == '\n' {
      Some(|next| next == '\n')
    } else {
      None
    };

    let Some(within) = gap else {
      at += 1;
      continue;
    };
    let end = at + characters[at..].iter().take_while(|(_, next)| within(*next)).count();

    gaps.push((offset(at), offset(end)));
    at = end;
  }

  gaps
}

// Where each sentence of the line starts and ends, its surrounding spaces left out.
fn sentence_spans(text: &str) -> Vec<(usize, usize)> {
  let mut spans = vec![];
  let mut start = 0;

  for (end, next) in sentence_gaps(text)
    .into_iter()
    .map(|(gap, next)| (gap, Some(next)))
    .chain([(text.len(), None)])
  {
    let piece = &text[start..end];

    if !js::trim(piece).is_empty() {
      let lead = piece.len() - piece.trim_start_matches(is_space).len();

      spans.push((start + lead, start + js::trim_end(piece).len()));
    }

    start = next.unwrap_or(end);
  }

  spans
}

// Korean for a sentence holding a Korean letter, English for one holding none with two words or more that
// have a lower-case letter, and none for one that says nothing by itself — a greeting, a model name, a
// figure.
fn said_language(sentence: &str) -> Option<Language> {
  if holds_korean(sentence) {
    return Some(Language::Ko);
  }

  let words = sentence
    .split(|character: char| !(character.is_ascii_alphabetic() || character == '\''))
    .filter(|word| word.chars().any(|character| character.is_ascii_lowercase()))
    .count();

  (words >= 2).then_some(Language::En)
}

/// The line as its stretches of one language each, in order (docs/voice-plan.md, 「Which sentences are
/// English」). A sentence that says nothing by itself takes the language of the next one that does, or of
/// the last one before it where none follows; a line where none does is English if it has a roman letter
/// and Korean otherwise. Neighbouring sentences of one language are one stretch, cut out of the line as they
/// stand.
pub fn voice_stretches(text: &str) -> Vec<VoiceStretch> {
  let spans = sentence_spans(text);
  let said = spans.iter().map(|&(start, end)| said_language(&text[start..end])).collect::<Vec<_>>();
  let fallback = if is_roman(text) { Language::En } else { Language::Ko };
  let mut stretches: Vec<VoiceStretch> = vec![];

  for (at, &(start, end)) in spans.iter().enumerate() {
    let language = said[at]
      .or_else(|| said[at + 1..].iter().find_map(|after| *after))
      .or_else(|| said[..at].iter().rev().find_map(|before| *before))
      .unwrap_or(fallback);

    match stretches.last_mut() {
      Some(last) if last.language == language => {
        last.end = end;
        last.text = text[last.start..end].to_owned();
      }
      _ => stretches.push(VoiceStretch {
        language,
        text: text[start..end].to_owned(),
        start,
        end,
      }),
    }
  }

  stretches
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
