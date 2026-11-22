//! Whether a line's spoken words say every word of its written words that is already written in the language's own
//! letters, in its place (docs/boundary-api.md, 「The spoken words」). Such a word is said as it is spelled, so it
//! stands in the spoken words as it stands in the written words; only its joins to what the spoken words rewrite
//! around it may change, and only as the rules change them. It only finds; the spoken words are a model's to write.

use super::numbers::CODE_MARKS;
use super::reading::Language;
use super::spoken::{CODE_CLOSE, CODE_OPEN, holds_korean, is_syllable, particle_forms};

/// A word of the written words that the spoken words leave out, or say out of its place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropped {
  pub word: String,
}

/// The words a language's spoken words keep as written, as the check reads them.
trait KeptWords: Sync {
  /// Every run of the written words the spoken words hold in order, each with the written word it stands in; a
  /// spelling an agreed reading of `agreed` reads otherwise is a name's, and no run.
  fn kept(&self, written: &str, agreed: &[(String, String)]) -> Vec<Kept>;
  /// The spoken words as the runs are looked for in them.
  fn plain(&self, spoken: &str) -> Vec<char>;
}

impl Language {
  fn kept_words(self) -> &'static dyn KeptWords {
    match self {
      Self::Ko => &Korean,
      Self::En => &English,
    }
  }
}

/// One run of the written words, as every form the spoken words may say it in, and the written word it stands in.
struct Kept {
  forms: Vec<Vec<Letter>>,
  word: String,
}

/// One letter of a run as the spoken words may say it.
#[derive(Clone, Copy)]
struct Letter {
  written: char,
  // The letter opens on a sound a name's reading before it may carry its final consonant into.
  onset_carried: bool,
  // The letter's final consonant may be carried onto a name's reading after it.
  final_carried: bool,
}

/// The first run of `written` that `spoken` does not say in its place, every run before it said, or none where the
/// spoken words say every one in order.
pub fn dropped(
  written: &str,
  spoken: &str,
  language: Language,
  agreed: &[(String, String)],
) -> Option<Dropped> {
  let words = language.kept_words();
  let spoken = words.plain(spoken);
  let mut from = 0;

  for kept in words.kept(written, agreed) {
    match kept.forms.iter().filter_map(|form| said_from(&spoken, from, form)).min() {
      Some(end) => from = end,
      None => return Some(Dropped { word: kept.word }),
    }
  }

  None
}

// Where the first saying of a run at or after `from` ends, the one that ends first.
fn said_from(spoken: &[char], from: usize, form: &[Letter]) -> Option<usize> {
  (from..=spoken.len().checked_sub(form.len())?)
    .find(|&at| spoken[at..at + form.len()].iter().zip(form).all(|(&said, letter)| letter.says(said)))
    .map(|at| at + form.len())
}

// The Hangul syllables are composed as (initial * 21 + medial) * 28 + final from the first syllable; the initial that
// writes no sound is the twelfth.
const FIRST_SYLLABLE: u32 = 0xAC00;
const FINALS: u32 = 28;
const SILENT_ONSET: u32 = 11;

// A syllable as its initial, its medial and its final.
fn parts(syllable: char) -> Option<(u32, u32, u32)> {
  let index = u32::from(syllable).checked_sub(FIRST_SYLLABLE).filter(|_| is_syllable(syllable))?;

  Some((index / (21 * FINALS), index / FINALS % 21, index % FINALS))
}

impl Letter {
  fn says(&self, said: char) -> bool {
    if said == self.written {
      return true;
    }

    let (Some((onset, medial, last)), Some((said_onset, said_medial, said_last))) =
      (parts(self.written), parts(said))
    else {
      return false;
    };

    medial == said_medial
      && (onset == said_onset || self.onset_carried)
      && (last == said_last || self.final_carried && said_last == 0)
  }
}

// Korean: every run of Hangul syllables of the written words.
struct Korean;

// Whether a word is code, an address or a path, which the spoken words say in a few words or leave out: one holding a
// slash, a mark of code, or a call's empty parentheses.
fn is_code(word: &[char]) -> bool {
  word.iter().any(|character| *character == '/' || CODE_MARKS.contains(character))
    || word.windows(2).any(|pair| pair == ['(', ')'])
}

// One character of the written words as the check reads it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Written {
  // A Hangul syllable the spoken words say as spelled.
  Syllable(char),
  // A letter of a name whose reading the spoken words write: a Latin letter, or a letter of a Hangul spelling an
  // agreed reading lists, which is said as that reading or as written.
  Name,
  // Anything else: a figure, a mark, punctuation.
  Other,
  Space,
}

impl Written {
  fn of(character: char) -> Self {
    if is_syllable(character) {
      Self::Syllable(character)
    } else if character.is_ascii_alphabetic() || ('\u{FF21}'..='\u{FF5A}').contains(&character) {
      Self::Name
    } else if character.is_whitespace() {
      Self::Space
    } else {
      Self::Other
    }
  }
}

// The written words read character by character, every span the script marks as code a space, since code is said for
// what it names, and every Hangul spelling of an agreed reading that says it otherwise a name.
fn read_written(written: &[char], agreed: &[(String, String)]) -> Vec<Written> {
  let mut within_code = false;
  let mut read = written
    .iter()
    .map(|&character| match character {
      CODE_OPEN => {
        within_code = true;
        Written::Space
      }
      CODE_CLOSE => {
        within_code = false;
        Written::Space
      }
      _ if within_code => Written::Space,
      _ => Written::of(character),
    })
    .collect::<Vec<_>>();
  let spaced = |text: &str| text.split_whitespace().collect::<String>();

  for (spelling, _) in agreed
    .iter()
    .filter(|(spelling, said)| holds_korean(spelling) && spaced(spelling) != spaced(said))
  {
    let spelling = spelling.chars().collect::<Vec<_>>();

    for at in 0..written.len().saturating_sub(spelling.len() - 1) {
      if written[at..at + spelling.len()] == spelling[..] {
        for letter in &mut read[at..at + spelling.len()] {
          if *letter != Written::Space {
            *letter = Written::Name;
          }
        }
      }
    }
  }

  read
}

impl KeptWords for Korean {
  fn kept(&self, written: &str, agreed: &[(String, String)]) -> Vec<Kept> {
    let characters = written.chars().collect::<Vec<_>>();
    let read = read_written(&characters, agreed);
    let mut kept = vec![];
    let mut at = 0;

    while at < read.len() {
      if read[at] == Written::Space {
        at += 1;
        continue;
      }

      let start = at;

      while at < read.len() && read[at] != Written::Space {
        at += 1;
      }

      if !is_code(&characters[start..at]) {
        let word = characters[start..at].iter().collect::<String>();

        kept.extend(runs(&read[start..at], &word));
      }
    }

    kept
  }

  fn plain(&self, spoken: &str) -> Vec<char> {
    spoken.chars().filter(|&character| is_syllable(character)).collect()
  }
}

// Every run of syllables of a written word, in every form it may be said in: a particle written onto a figure, a name
// or a mark fitted to how they are said, a name's final consonant carried into a run that opens on no sound, and a
// run's own final carried onto a name after it.
fn runs(word: &[Written], written: &str) -> Vec<Kept> {
  let named = written.trim_matches(|character: char| !character.is_alphanumeric()).to_owned();
  let mut kept = vec![];
  let mut at = 0;

  while at < word.len() {
    let start = at;

    while let Some(Written::Syllable(_)) = word.get(at) {
      at += 1;
    }

    if start == at {
      at += 1;
      continue;
    }

    let syllables = word[start..at]
      .iter()
      .filter_map(|letter| match letter {
        Written::Syllable(syllable) => Some(*syllable),
        _ => None,
      })
      .collect::<String>();
    let after = start.checked_sub(1).map(|before| word[before]);
    let after_name = after == Some(Written::Name);
    let before_name = word.get(at) == Some(&Written::Name);
    let spellings = if after.is_some() { particle_forms(&syllables) } else { vec![syllables] };

    let forms = spellings
      .iter()
      .map(|spelling| {
        let count = spelling.chars().count();

        spelling
          .chars()
          .enumerate()
          .map(|(at, written)| Letter {
            written,
            onset_carried: after_name
              && at == 0
              && parts(written).is_some_and(|(onset, ..)| onset == SILENT_ONSET),
            final_carried: before_name && at + 1 == count,
          })
          .collect()
      })
      .collect();

    kept.push(Kept {
      forms,
      word: named.clone(),
    });
  }

  kept
}

// English: a Korean word of an English line is written in English letters, and its own words are not looked for.
struct English;

impl KeptWords for English {
  fn kept(&self, _: &str, _: &[(String, String)]) -> Vec<Kept> {
    vec![]
  }

  fn plain(&self, spoken: &str) -> Vec<char> {
    spoken.chars().collect()
  }
}
