//! Whether a line's spoken words say every number its written words hold as it is written (docs/boundary-api.md,
//! 「The spoken words」). A number has several readings that are right where it stands — a count, an hour, figure
//! by figure — and only its context picks among them, so the check takes any reading the line's language gives a
//! number and finds one the spoken words say in none. It only finds; the spoken words are a model's to write.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

use super::figures;
use super::reading::Language;
use super::spoken::is_syllable;
use super::words::{FigureWords, VOICE_SCRIPT};

/// A number of the written words that the spoken words do not say, and the reading the language's rules give it
/// where it stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Misread {
  pub written: String,
  pub said: String,
}

/// The words a language says numbers in, as the check reads them. Every reading is matched once made plain.
trait NumberWords: Sync {
  /// Text as readings are matched in it: the letters the language writes alone, its spaces and marks gone.
  fn plain(&self, text: &str) -> String;
  /// Every reading of a whole number's figures where it stands before `after`; an empty one where the words
  /// after it say the number already.
  fn whole(&self, figures: &str, after: &str) -> Vec<String>;
  /// Every reading of one part of a decimal or a version: figure by figure, or whole.
  fn dotted_part(&self, figures: &str) -> Vec<String>;
  /// Every way a part said already and the part after it are joined by the point between them.
  fn pointed(&self, head: &str, tail: &str) -> Vec<String>;
  /// The words a percent sign is said in.
  fn percent(&self) -> Vec<String>;
  /// The reading the language's rules give a number where it stands, named when the spoken words miss it.
  fn said(&self, written: &str, after: &str) -> String;
}

impl Language {
  fn number_words(self) -> &'static dyn NumberWords {
    match self {
      Self::Ko => &Korean,
      Self::En => &English,
    }
  }
}

// A whole number with or without thousands commas, the parts after each point, and a percent sign after them.
static NUMBER: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?:[0-9]{1,3}(?:,[0-9]{3})+|[0-9]+)(?:\.[0-9]+)*%?").expect("a number"));
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\S+").expect("a word"));

// The marks code is written with; a word holding one is code, said in a few words or left out.
const CODE: &[char] = &['`', '_', '=', '@', '<', '>', '{', '}', '[', ']', '|', '\\', '^', '&', '*'];

// More parts than this make a version too long to spell out every reading of; it is read by its parts alone.
const MOST_JOINED_PARTS: usize = 4;

// A number as the written words hold it.
struct Number<'a> {
  written: &'a str,
  // The figures of each part a point divides it into, its thousands commas taken out.
  parts: Vec<String>,
  percent: bool,
  after: &'a str,
}

// A word of the written words whose figures are a name's or code's: written onto a Latin letter, among the marks
// of code, or a private mark the script makes.
fn names_or_code(word: &str) -> bool {
  word
    .chars()
    .any(|character| character.is_ascii_alphabetic() || CODE.contains(&character) || is_private(character))
}

fn is_private(character: char) -> bool {
  ('\u{E000}'..='\u{F8FF}').contains(&character)
}

// Whether a whole number may be said figure by figure: one opening with 0, or of three figures or more, as a code,
// a phone number or a model's number is. A shorter one is said as a number, so its figures alone do not say it.
fn by_figure(figures: &str) -> bool {
  figures.starts_with('0') || figures.len() >= 3
}

// Every number of the written words the spoken words must say.
fn numbers(text: &str) -> Vec<Number<'_>> {
  let mut found = vec![];

  for word in WORD.find_iter(text).filter(|word| !names_or_code(word.as_str())) {
    for number in NUMBER.find_iter(word.as_str()) {
      let written = number.as_str();
      let figures = written.trim_end_matches('%');

      found.push(Number {
        written,
        parts: figures.split('.').map(|part| part.replace(',', "")).collect(),
        percent: figures.len() < written.len(),
        after: &text[word.start() + number.end()..],
      });
    }
  }

  found
}

// Every reading of a number as a whole, plain.
fn readings(words: &dyn NumberWords, number: &Number<'_>) -> BTreeSet<String> {
  let Some((head, tails)) = number.parts.split_first() else {
    return BTreeSet::new();
  };

  if tails.is_empty() {
    return words.whole(head, number.after).iter().map(|reading| words.plain(reading)).collect();
  }

  if number.parts.len() > MOST_JOINED_PARTS {
    return BTreeSet::new();
  }

  let mut runs = words.dotted_part(head).into_iter().collect::<BTreeSet<_>>();

  for tail in tails {
    let said = words.dotted_part(tail);

    runs = runs
      .iter()
      .flat_map(|run| said.iter().flat_map(|part| words.pointed(run, part)))
      .collect();
  }

  runs.iter().map(|reading| words.plain(reading)).collect()
}

// Whether the plain spoken words say a number in one of its readings, a percent sign's word after it where it has
// one. A run of three parts or more is also said where each part is said whole, as a date's parts are.
fn is_said(words: &dyn NumberWords, number: &Number<'_>, spoken: &str) -> bool {
  let percent = words.percent().iter().map(|word| words.plain(word)).collect::<Vec<_>>();
  let holds = |reading: &String| {
    if number.percent {
      percent.iter().any(|word| spoken.contains(&format!("{reading}{word}")))
    } else {
      reading.is_empty() || spoken.contains(reading.as_str())
    }
  };

  readings(words, number).iter().any(holds)
    || number.parts.len() >= 3
      && number.parts.iter().all(|part| {
        words
          .whole(part, "")
          .iter()
          .any(|reading| spoken.contains(words.plain(reading).as_str()))
      })
}

/// The first number of `written` that `spoken` does not say in any reading the language gives it, or none where
/// the spoken words say every one.
pub fn misread(written: &str, spoken: &str, language: Language) -> Option<Misread> {
  let words = language.number_words();
  let spoken = words.plain(spoken);

  numbers(written)
    .into_iter()
    .find(|number| !is_said(words, number, &spoken))
    .map(|number| Misread {
      written: number.written.to_owned(),
      said: words.said(number.written, number.after),
    })
}

// Korean numbers, in the words of the language's data file.
struct Korean;

fn figure_words() -> &'static FigureWords {
  &VOICE_SCRIPT.figures
}

// Figures one by one with zero said as its other word.
fn spelled_with_zero(figures: &str) -> String {
  let zero = figure_words().digits.first().map_or("", String::as_str);

  figures::spelled(figures).replace(zero, &figure_words().zero)
}

// Figures one by one as a name's figures are said in English.
fn spelled_in_english(figures: &str) -> String {
  figures
    .chars()
    .filter_map(|figure| figure.to_digit(10))
    .filter_map(|digit| figure_words().english_digits.get(digit as usize))
    .map(String::as_str)
    .collect()
}

// The final consonant closing `head` carried onto `tail` where tail opens on a vowel, as a run said in one breath
// sounds; none where there is nothing to carry.
fn carried(head: &str, tail: &str) -> Option<String> {
  // The Hangul syllables are composed as (initial * 21 + medial) * 28 + final from the first syllable; the initial
  // that writes no sound is the twelfth, and each simple final consonant has an initial of the same sound.
  const FIRST: u32 = 0xAC00;
  const SILENT: u32 = 11;
  const ONSETS: [(u32, u32); 14] = [
    (1, 0),
    (2, 1),
    (4, 2),
    (7, 3),
    (8, 5),
    (16, 6),
    (17, 7),
    (19, 9),
    (20, 10),
    (22, 12),
    (23, 14),
    (24, 15),
    (25, 16),
    (26, 17),
  ];

  let last = head.chars().last().filter(|&last| is_syllable(last))? as u32 - FIRST;
  let first = tail.chars().next().filter(|&first| is_syllable(first))? as u32 - FIRST;
  let onset = ONSETS.iter().find(|(closing, _)| *closing == last % 28)?.1;

  if first / (21 * 28) != SILENT {
    return None;
  }

  let closed = char::from_u32(FIRST + last - last % 28)?;
  let opened = char::from_u32(FIRST + first - SILENT * 21 * 28 + onset * 21 * 28)?;
  let head = head.strip_suffix(|_: char| true)?;
  let tail = tail.strip_prefix(|_: char| true)?;

  Some(format!("{head}{closed}{opened}{tail}"))
}

impl NumberWords for Korean {
  fn plain(&self, text: &str) -> String {
    text.chars().filter(|&character| is_syllable(character)).collect()
  }

  fn whole(&self, figures: &str, after: &str) -> Vec<String> {
    let words = figure_words();
    let trimmed = figures.trim_start_matches('0');

    // A counter that asks a reading of its own is said with that reading alone, an hour also on a twelve-hour clock.
    if let Some(counted) = figures::counted(trimmed, after) {
      let mut said = vec![counted];

      if let Ok(n @ 13..=24) = trimmed.parse::<u64>()
        && after.trim_start().starts_with(words.hour.as_str())
      {
        said.push(figures::native(&(n - 12).to_string()));
      }

      return said;
    }

    let mut said = vec![figures::sino_korean(figures), spelled_in_english(figures)];

    if by_figure(figures) {
      said.extend([figures::spelled(figures), spelled_with_zero(figures)]);
    }

    let Ok(n) = trimmed.parse::<u64>() else {
      return said;
    };

    said.push(figures::sino_korean(trimmed));

    if trimmed.len() <= 15 {
      said.push(figures::native(trimmed));
      said.push(figures::native_alone(trimmed));
    }

    if n == 1 {
      said.push(words.first.clone());

      // One ten thousand, one hundred million and the like are said as the group alone.
      if words
        .groups
        .iter()
        .chain(&words.places)
        .any(|place| !place.is_empty() && after.starts_with(place.as_str()))
      {
        said.push(String::new());
      }
    }

    // The hour of a clock time, said on a twelve-hour clock.
    if (13..=24).contains(&n) && after.starts_with(':') {
      said.push(figures::native(&(n - 12).to_string()));
    }

    said
  }

  fn dotted_part(&self, figures: &str) -> Vec<String> {
    vec![
      figures::spelled(figures),
      spelled_with_zero(figures),
      figures::sino_korean(figures),
      spelled_in_english(figures),
    ]
  }

  fn pointed(&self, head: &str, tail: &str) -> Vec<String> {
    let words = figure_words();

    [&words.point, &words.point_tensed]
      .into_iter()
      .flat_map(|point| {
        let joined = format!("{head}{point}");
        let carried = carried(&joined, tail);

        [Some(format!("{joined}{tail}")), carried]
      })
      .flatten()
      .collect()
  }

  fn percent(&self) -> Vec<String> {
    let words = figure_words();

    words
      .symbols
      .get("%")
      .map(|word| word.trim().to_owned())
      .into_iter()
      .chain(words.percent_also.clone())
      .collect()
  }

  fn said(&self, written: &str, after: &str) -> String {
    figures::said_run(written, after)
  }
}

// English numbers, in the English words for them.
struct English;

const ONES: [&str; 20] = [
  "zero",
  "one",
  "two",
  "three",
  "four",
  "five",
  "six",
  "seven",
  "eight",
  "nine",
  "ten",
  "eleven",
  "twelve",
  "thirteen",
  "fourteen",
  "fifteen",
  "sixteen",
  "seventeen",
  "eighteen",
  "nineteen",
];
const TENS: [&str; 10] =
  ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
const SCALES: [&str; 5] = ["", "thousand", "million", "billion", "trillion"];
// Zero as it is also said figure by figure.
const OH: &str = "oh";

fn below_hundred(n: u64) -> String {
  match n {
    0..20 => ONES[n as usize].to_owned(),
    _ if n.is_multiple_of(10) => TENS[(n / 10) as usize].to_owned(),
    _ => format!("{}-{}", TENS[(n / 10) as usize], ONES[(n % 10) as usize]),
  }
}

// Below a thousand, an `and` before the tens of a hundred where `and` holds.
fn below_thousand(n: u64, and: bool) -> String {
  let (hundreds, rest) = (n / 100, n % 100);

  match (hundreds, rest) {
    (0, _) => below_hundred(rest),
    (_, 0) => format!("{} hundred", ONES[hundreds as usize]),
    _ if and => format!("{} hundred and {}", ONES[hundreds as usize], below_hundred(rest)),
    _ => format!("{} hundred {}", ONES[hundreds as usize], below_hundred(rest)),
  }
}

// A whole number below a thousand trillion in words.
fn cardinal(n: u64, and: bool) -> String {
  if n == 0 {
    return ONES[0].to_owned();
  }

  let mut said = vec![];
  let mut rest = n;

  for scale in SCALES {
    let group = rest % 1000;

    if group != 0 {
      said.push(if scale.is_empty() {
        below_thousand(group, and)
      } else {
        format!("{} {scale}", below_thousand(group, and))
      });
    }

    rest /= 1000;
  }

  said.reverse();
  said.join(" ")
}

fn digit_by_digit(figures: &str, zero: &str) -> String {
  figures
    .chars()
    .filter_map(|figure| figure.to_digit(10))
    .map(|digit| if digit == 0 { zero } else { ONES[digit as usize] })
    .collect::<Vec<_>>()
    .join(" ")
}

// A four-figure number said as a year is, by its two pairs of figures.
fn as_year(n: u64) -> String {
  let (high, low) = (n / 100, n % 100);
  let low = match low {
    0 => "hundred".to_owned(),
    1..10 => format!("{OH} {}", ONES[low as usize]),
    _ => below_hundred(low),
  };

  format!("{} {low}", below_hundred(high))
}

// The largest number English words are made for here; a longer one is said figure by figure.
const MOST_CARDINAL: u64 = 999_999_999_999_999;

impl NumberWords for English {
  fn plain(&self, text: &str) -> String {
    text
      .chars()
      .filter(char::is_ascii_alphabetic)
      .map(|letter| letter.to_ascii_lowercase())
      .collect()
  }

  fn whole(&self, figures: &str, after: &str) -> Vec<String> {
    let mut said = vec![];

    if by_figure(figures) {
      said.extend([digit_by_digit(figures, ONES[0]), digit_by_digit(figures, OH)]);
    }

    let Some(n) = figures.parse::<u64>().ok().filter(|&n| n <= MOST_CARDINAL) else {
      return said;
    };

    said.extend([cardinal(n, false), cardinal(n, true)]);

    if (1000..10_000).contains(&n) {
      said.push(as_year(n));
    }

    // An hour of a twenty-four-hour clock, said on a twelve-hour one.
    if (13..=24).contains(&n) && after.starts_with(':') {
      said.push(cardinal(n - 12, false));
    }

    said
  }

  fn dotted_part(&self, figures: &str) -> Vec<String> {
    let mut said = vec![digit_by_digit(figures, ONES[0]), digit_by_digit(figures, OH)];

    said.extend(figures.parse::<u64>().ok().filter(|&n| n <= MOST_CARDINAL).map(|n| cardinal(n, false)));
    said
  }

  fn pointed(&self, head: &str, tail: &str) -> Vec<String> {
    vec![format!("{head} point {tail}"), format!("{head} dot {tail}")]
  }

  fn percent(&self) -> Vec<String> {
    vec!["percent".to_owned()]
  }

  fn said(&self, written: &str, _: &str) -> String {
    let figures = written.trim_end_matches('%').replace(',', "");
    let mut parts = figures.split('.');
    let whole = parts.next().unwrap_or_default();
    let whole = whole
      .parse::<u64>()
      .ok()
      .filter(|&n| n <= MOST_CARDINAL)
      .map_or_else(|| digit_by_digit(whole, ONES[0]), |n| cardinal(n, false));
    let mut said =
      parts.fold(whole, |said, part| format!("{said} point {}", digit_by_digit(part, ONES[0])));

    if figures.len() < written.len() {
      said.push_str(" percent");
    }

    said
  }
}
