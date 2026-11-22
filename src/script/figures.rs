//! Figures said by rule before the reading gate sees a Korean stretch (docs/voice-plan.md, 「How figures are
//! said」).
//!
//! The gate is a model, and a model reads a figure by guessing at it. A figure has one right reading where it
//! stands — a count, a date, a time, an item number, a version — and that reading can be worked out from the
//! figure and the word after it, so it is worked out here and the gate is left only the figures that belong
//! to a roman name, which it reads with the name. The words a figure is read in are the language's data file's.

use std::sync::LazyLock;

use bmo_text::js::SPACE;
use regex::Regex;

use super::spoken::{HANGUL, fitted_particle, is_syllable, standing_particle};
use super::words::{FigureWords, VOICE_SCRIPT, fill};

fn words() -> &'static FigureWords {
  &VOICE_SCRIPT.figures
}

// The word a figure from 0 to 9 is said in, one by one or in Sino-Korean.
fn digit_word(figure: char) -> &'static str {
  figure
    .to_digit(10)
    .and_then(|at| words().digits.get(at as usize))
    .map_or("", String::as_str)
}

// Figures one by one: a number opening with 0, and the figures after a decimal point.
pub(crate) fn spelled(figures: &str) -> String {
  figures.chars().map(digit_word).collect()
}

// One group of four figures in Sino-Korean, the one before the tens, hundreds and thousands left out.
fn group_word(four: &str) -> String {
  let padded = format!("{four:0>4}");

  padded
    .chars()
    .enumerate()
    .map(|(at, figure)| {
      let place = 3 - at;

      if figure == '0' {
        return String::new();
      }

      let digit = if figure == '1' && place > 0 { "" } else { digit_word(figure) };

      format!("{digit}{}", words().places.get(place).map_or("", String::as_str))
    })
    .collect()
}

/// A whole number in Sino-Korean, grouped by tens of thousands with a space between groups. One opening
/// with 0, or too long to group, is said figure by figure.
pub fn sino_korean(figures: &str) -> String {
  let groups = &words().groups;

  if !figures.is_empty() && figures.chars().all(|figure| figure == '0') {
    return digit_word('0').to_owned();
  }

  if figures.starts_with('0') || figures.len() > groups.len() * 4 {
    return spelled(figures);
  }

  let mut said = vec![];
  let mut end = figures.len();
  let mut group = 0;

  while end > 0 {
    let four = &figures[end.saturating_sub(4)..end];
    let value = four.parse::<u64>().unwrap_or(0);

    if value != 0 {
      // Ten thousand alone rather than one ten thousand; the groups above keep their one.
      let name = groups.get(group).map_or("", String::as_str);

      said.push(if group == 1 && value == 1 {
        name.to_owned()
      } else {
        format!("{}{name}", group_word(four))
      });
    }

    end = end.saturating_sub(4);
    group += 1;
  }

  said.reverse();
  said.join(" ")
}

// A count of one to ninety-nine in the native form a counter takes.
fn native_count(n: u64) -> String {
  let (tens, ones) = ((n / 10) as usize, (n % 10) as usize);

  if tens == 2 && ones == 0 {
    return words().twenty.clone();
  }

  let tens = words().native_tens.get(tens).map_or("", String::as_str);

  format!("{tens}{}", words().native_ones.get(ones).map_or("", String::as_str))
}

// A count of one to ninety-nine in the native form it is said in alone.
fn native_count_alone(n: u64) -> String {
  let (tens, ones) = ((n / 10) as usize, (n % 10) as usize);
  let tens = words().native_tens.get(tens).map_or("", String::as_str);

  format!("{tens}{}", words().native_ones_alone.get(ones).map_or("", String::as_str))
}

// How a whole number is said before the word that follows it.
type Reading = fn(&str) -> String;

// A figure written with leading zeros, as the number it is.
fn trimmed(figures: &str) -> String {
  figures.parse::<u64>().map_or_else(|_| figures.to_owned(), |n| n.to_string())
}

// A native count: native Korean up to 99, the hundreds above it in Sino-Korean.
pub(crate) fn native(figures: &str) -> String {
  native_by(figures, native_count)
}

/// A native count as it is said alone, with no counter after it.
pub(crate) fn native_alone(figures: &str) -> String {
  native_by(figures, native_count_alone)
}

// A native count, its last two figures in the native form `count` says them in.
fn native_by(figures: &str, count: fn(u64) -> String) -> String {
  if figures.starts_with('0') || figures.len() > 15 {
    return sino_korean(figures);
  }

  let n = figures.parse::<u64>().unwrap_or(0);
  let rest = n % 100;
  let hundreds = if n == rest { String::new() } else { sino_korean(&(n - rest).to_string()) };

  format!("{hundreds}{}", if rest == 0 { String::new() } else { count(rest) })
}

fn ordinal(figures: &str) -> String {
  if figures == "1" { words().first.clone() } else { native(figures) }
}

fn month(figures: &str) -> String {
  words().months.get(figures).cloned().unwrap_or_else(|| sino_korean(figures))
}

// An hour is native only from 1 to 12, as a twelve-hour clock is read; 0 and 13 to 24 are a
// twenty-four-hour clock, read in Sino-Korean.
fn hour(figures: &str) -> String {
  if figures.len() > 2 {
    return sino_korean(figures);
  }

  let n = figures.parse::<u64>().unwrap_or(0);

  if (1..=12).contains(&n) {
    native(&trimmed(figures))
  } else {
    sino_korean(&trimmed(figures))
  }
}

// The machine counter counts natively below ten; above it the figure is an age, said in Sino-Korean.
fn few_native(figures: &str) -> String {
  if figures.len() == 1 { native(figures) } else { sino_korean(figures) }
}

// A word that changes how the figure before it is said, and whether it must end its word to be one.
struct Counter {
  word: &'static str,
  reading: Reading,
  bounded: bool,
}

// The counters that change how the figure before them is said; every other word takes the Sino-Korean
// reading. The longest word is tried first, so a counter holding a shorter one at its start is taken whole.
static COUNTERS: LazyLock<Vec<Counter>> = LazyLock::new(|| {
  let figures = words();
  let natives = figures.native_counters.iter().map(|word| Counter {
    word,
    reading: native,
    bounded: true,
  });
  let others = [
    (&figures.hour, hour as Reading, true),
    (&figures.ordinal, ordinal, true),
    (&figures.machines, few_native, true),
    (&figures.month, month, false),
    (&figures.months_counted, sino_korean, false),
  ]
  .map(|(word, reading, bounded)| Counter {
    word,
    reading,
    bounded,
  });
  let mut counters = natives.chain(others).collect::<Vec<_>>();

  counters.sort_by_key(|counter| std::cmp::Reverse(counter.word.chars().count()));
  counters
});

// What may follow a bounded counter for it to be one: the end of the word, or a particle or an ending.
fn ends_counter(rest: &str) -> bool {
  rest
    .chars()
    .next()
    .is_none_or(|next| !is_syllable(next) || words().counter_ends.contains(next))
}

// The counter a figure stands before, directly or after one space, and how it asks the figure to be said;
// none where the word after it is no counter.
fn counter_reading(after: &str) -> Option<Reading> {
  let word = after.strip_prefix(' ').unwrap_or(after);

  COUNTERS
    .iter()
    .find(|counter| {
      word
        .strip_prefix(counter.word)
        .is_some_and(|rest| !counter.bounded || ends_counter(rest))
    })
    .map(|counter| counter.reading)
}

/// A whole number said the way the counter after it asks, directly or one space after it; none where the word
/// after it is no counter.
pub(crate) fn counted(figures: &str, after: &str) -> Option<String> {
  counter_reading(after).map(|reading| reading(figures))
}

// A decimal as its whole part, the point, and the figures after the point one by one; three parts or more
// are a version, each part a whole number.
fn dotted_reading(head: &str, tail: &str) -> String {
  let point = &words().point;
  let parts = tail[1..].split('.').collect::<Vec<_>>();

  if let [only] = parts.as_slice() {
    return format!("{} {point} {}", sino_korean(head), spelled(only));
  }

  [head]
    .iter()
    .chain(&parts)
    .map(|part| sino_korean(part))
    .collect::<Vec<_>>()
    .join(&format!(" {point} "))
}

// One piece of a run of figures: how it is said and how many characters of the run it takes.
struct Piece {
  said: String,
  length: usize,
}

// Where in its run a piece is read, and what follows the run.
struct RunPlace<'a> {
  run: &'a [char],
  at: usize,
  after: &'a str,
}

type PieceReader = fn(&[char], &RunPlace) -> Option<Piece>;

fn is_figure(rest: &[char], at: usize) -> bool {
  rest.get(at).is_some_and(char::is_ascii_digit)
}

// How many figures stand in a row from `at`.
fn figures_at(rest: &[char], at: usize) -> usize {
  rest
    .get(at..)
    .map_or(0, |tail| tail.iter().take_while(|figure| figure.is_ascii_digit()).count())
}

fn text(rest: &[char], from: usize, to: usize) -> String {
  rest[from..to].iter().collect()
}

// Where a number written with thousands commas may end, the most groups first: one to three figures and
// at least one comma with three figures after it.
fn grouped_ends(rest: &[char]) -> Vec<usize> {
  let lead = figures_at(rest, 0);
  let mut ends = vec![];

  if !(1..=3).contains(&lead) {
    return ends;
  }

  let mut at = lead;

  while rest.get(at) == Some(&',') && (1..=3).all(|step| is_figure(rest, at + step)) {
    at += 4;
    ends.push(at);
  }

  ends.reverse();
  ends
}

// The length of a whole number at the start: written with thousands commas where no figure follows them,
// else its figures.
fn whole_length(rest: &[char]) -> Option<usize> {
  grouped_ends(rest)
    .into_iter()
    .find(|&end| !is_figure(rest, end))
    .or_else(|| Some(figures_at(rest, 0)).filter(|&length| length > 0))
}

// The end of the decimal parts from `at`: each a point and its figures, as many as stand.
fn decimals_end(rest: &[char], at: usize) -> usize {
  let mut end = at;

  while rest.get(end) == Some(&'.') && is_figure(rest, end + 1) {
    end += 1 + figures_at(rest, end + 1);
  }

  end
}

fn sign(rest: &[char], place: &RunPlace) -> Option<Piece> {
  let mark = *rest.first()?;

  if !matches!(mark, '-' | '\u{2212}' | '+') || !is_figure(rest, 1) || place.at > 0 {
    return None;
  }

  let word = if mark == '+' { &words().plus } else { &words().minus };

  Some(Piece {
    said: format!("{word} "),
    length: 1,
  })
}

// A date, year first, its parts apart by one mark.
fn date(rest: &[char], _: &RunPlace) -> Option<Piece> {
  let mark = *rest.get(4)?;

  if figures_at(rest, 0) != 4 || !matches!(mark, '-' | '.' | '/') {
    return None;
  }

  let month_length = figures_at(rest, 5);
  let day_at = 6 + month_length;
  let day_length = figures_at(rest, day_at);

  if !(1..=2).contains(&month_length)
    || rest.get(5 + month_length) != Some(&mark)
    || !(1..=2).contains(&day_length)
  {
    return None;
  }

  let (mm, dd) = (text(rest, 5, 5 + month_length), text(rest, day_at, day_at + day_length));
  let (month_number, day_number) = (mm.parse::<u64>().ok()?, dd.parse::<u64>().ok()?);

  if !(1..=12).contains(&month_number) || !(1..=31).contains(&day_number) {
    return None;
  }

  let said = fill(
    &words().date,
    &[
      ("year", &sino_korean(&text(rest, 0, 4))),
      ("month", &month(&trimmed(&mm))),
      ("day", &sino_korean(&trimmed(&dd))),
    ],
  );

  Some(Piece {
    said,
    length: day_at + day_length,
  })
}

// A clock time: the hour as an hour is read, minutes and seconds in Sino-Korean.
fn clock(rest: &[char], _: &RunPlace) -> Option<Piece> {
  let hour_length = figures_at(rest, 0);

  if !(1..=2).contains(&hour_length) || rest.get(hour_length) != Some(&':') {
    return None;
  }

  let minutes_at = hour_length + 1;

  if !is_figure(rest, minutes_at) || !is_figure(rest, minutes_at + 1) {
    return None;
  }

  let after_minutes = minutes_at + 2;
  let closes = |at: usize| !is_figure(rest, at) && rest.get(at) != Some(&':');
  let with_seconds = rest.get(after_minutes) == Some(&':')
    && is_figure(rest, after_minutes + 1)
    && is_figure(rest, after_minutes + 2)
    && closes(after_minutes + 3);

  if !with_seconds && !closes(after_minutes) {
    return None;
  }

  let hh = text(rest, 0, hour_length);
  let mm = text(rest, minutes_at, after_minutes);
  let ss = if with_seconds {
    text(rest, after_minutes + 1, after_minutes + 3)
  } else {
    "0".to_owned()
  };
  let [hours, minutes, seconds] = [&hh, &mm, &ss].map(|part| part.parse::<u64>().unwrap_or(0));

  if hours > 24 || minutes > 59 || seconds > 59 {
    return None;
  }

  let figures = words();
  let mut said = vec![fill(&figures.hours, &[("hours", &hour(&hh))])];

  if minutes > 0 {
    said.push(fill(&figures.minutes, &[("minutes", &sino_korean(&trimmed(&mm)))]));
  }

  if seconds > 0 {
    said.push(fill(&figures.seconds, &[("seconds", &sino_korean(&trimmed(&ss)))]));
  }

  Some(Piece {
    said: said.join(" "),
    length: if with_seconds { after_minutes + 3 } else { after_minutes },
  })
}

// Two figures a colon apart that make no time.
fn ratio(rest: &[char], _: &RunPlace) -> Option<Piece> {
  let left = figures_at(rest, 0);
  let right = figures_at(rest, left + 1);

  if left == 0 || rest.get(left) != Some(&':') || right == 0 {
    return None;
  }

  let said = fill(
    &words().ratio,
    &[
      ("left", &sino_korean(&text(rest, 0, left))),
      ("right", &sino_korean(&text(rest, left + 1, left + 1 + right))),
    ],
  );

  Some(Piece {
    said,
    length: left + 1 + right,
  })
}

// An item number with its counter word, unless it is written with one already.
fn item(rest: &[char], place: &RunPlace) -> Option<Piece> {
  let figures = figures_at(rest, 1);

  if rest.first() != Some(&'#') || figures == 0 {
    return None;
  }

  let word = &words().item;
  let length = 1 + figures;
  let written = place.at + length == place.run.len() && place.after.starts_with(word.as_str());

  Some(Piece {
    said: format!("{}{}", sino_korean(&text(rest, 1, length)), if written { "" } else { word }),
    length,
  })
}

fn fraction(rest: &[char], _: &RunPlace) -> Option<Piece> {
  let top = figures_at(rest, 0);
  let bottom = figures_at(rest, top + 1);
  let end = top + 1 + bottom;

  if top == 0 || rest.get(top) != Some(&'/') || bottom == 0 || rest.get(end) == Some(&'/') {
    return None;
  }

  let said = fill(
    &words().fraction,
    &[
      ("top", &sino_korean(&text(rest, 0, top))),
      ("bottom", &sino_korean(&text(rest, top + 1, end))),
    ],
  );

  Some(Piece { said, length: end })
}

fn currency(rest: &[char], _: &RunPlace) -> Option<Piece> {
  let template = match rest.first()? {
    '$' => &words().dollars,
    '\u{20A9}' => &words().won,
    _ => return None,
  };
  let amount_end = 1 + whole_length(&rest[1..])?;
  let decimals_end = if rest.get(amount_end) == Some(&'.') && is_figure(rest, amount_end + 1) {
    amount_end + 1 + figures_at(rest, amount_end + 1)
  } else {
    amount_end
  };
  let figures = text(rest, 1, amount_end).replace(',', "");
  let amount = if decimals_end == amount_end {
    sino_korean(&figures)
  } else {
    dotted_reading(&figures, &text(rest, amount_end, decimals_end))
  };

  Some(Piece {
    said: fill(template, &[("amount", &amount)]),
    length: decimals_end,
  })
}

// A decimal, or a version of three parts or more.
fn decimal(rest: &[char], _: &RunPlace) -> Option<Piece> {
  let opens_decimals = |end: usize| rest.get(end) == Some(&'.') && is_figure(rest, end + 1);
  let head_end = grouped_ends(rest)
    .into_iter()
    .find(|&end| opens_decimals(end))
    .or_else(|| Some(figures_at(rest, 0)).filter(|&length| length > 0 && opens_decimals(length)))?;
  let end = decimals_end(rest, head_end);

  Some(Piece {
    said: dotted_reading(&text(rest, 0, head_end).replace(',', ""), &text(rest, head_end, end)),
    length: end,
  })
}

// What stays of a run after a figure that is the last one of it, or of a range closing it.
fn only_range_after(rest: &[char]) -> bool {
  let mut at = 0;

  while at < rest.len() {
    if !matches!(rest[at], '~' | '\u{223C}') || !is_figure(rest, at + 1) {
      return false;
    }

    at += 2;
    at += rest[at..]
      .iter()
      .take_while(|figure| figure.is_ascii_digit() || **figure == ',')
      .count();
  }

  true
}

// A whole number, counted the way the counter after the run asks where it is the run's last.
fn whole(rest: &[char], place: &RunPlace) -> Option<Piece> {
  let length = whole_length(rest)?;
  let figures = text(rest, 0, length).replace(',', "");
  let last = only_range_after(&place.run[place.at + length..]);
  let reading = last.then(|| counter_reading(place.after)).flatten().unwrap_or(sino_korean);

  Some(Piece {
    said: reading(&figures),
    length,
  })
}

// A tilde between two figures is a range.
fn range(rest: &[char], place: &RunPlace) -> Option<Piece> {
  let between = matches!(rest.first(), Some('~' | '\u{223C}')) && is_figure(rest, 1) && place.at > 0;

  between.then(|| Piece {
    said: format!("{} ", words().range),
    length: 1,
  })
}

// Any other mark: a symbol that is read, or the mark as written.
fn mark(rest: &[char], _: &RunPlace) -> Option<Piece> {
  let mark = rest.first()?.to_string();

  Some(Piece {
    said: words().symbols.get(&mark).cloned().unwrap_or(mark),
    length: 1,
  })
}

// The pieces a run is read out of, tried in this order at each place in it.
const PIECES: [PieceReader; 11] =
  [sign, date, clock, ratio, item, fraction, currency, decimal, whole, range, mark];

/// A run of figures and the marks it is written with, said the way these rules read it before `after`.
pub(crate) fn said_run(run: &str, after: &str) -> String {
  read_run(&run.chars().collect::<Vec<_>>(), after)
}

// One run of figures and the marks it is written with, said, given what follows the run.
fn read_run(run: &[char], after: &str) -> String {
  let mut out = String::new();
  let mut at = 0;

  while at < run.len() {
    let place = RunPlace { run, at, after };
    let rest = &run[at..];
    let piece = PIECES.iter().find_map(|reader| reader(rest, &place)).unwrap_or(Piece {
      said: rest.iter().collect(),
      length: rest.len(),
    });

    out.push_str(&piece.said);
    at += piece.length;
  }

  out
}

// A run of the stretch that is neither Hangul nor white space nor a title's particle — the same cut the
// reading gate's spans are made of.
pub(crate) static RUN: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(&format!(r"[^{SPACE}{HANGUL}\x{{E000}}]+")).expect("a run of figures"));
static CODE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\x{E001}[^\x{E002}]*\x{E002}?").expect("a piece of code"));

fn is_roman(text: &str) -> bool {
  text.chars().any(|character| character.is_ascii_alphabetic())
}

fn spaces_only(text: &str) -> bool {
  !text.is_empty() && text.chars().all(|character| character == ' ' || character == '\t')
}

// A piece of the stretch outside code, with every figure standing on its own said.
fn say_figures_in(text: &str) -> String {
  let runs = RUN.find_iter(text).collect::<Vec<_>>();
  let mut out = String::new();
  let mut from = 0;

  for (at, run) in runs.iter().enumerate() {
    if !run.as_str().chars().any(|character| character.is_ascii_digit()) || is_roman(run.as_str()) {
      continue;
    }

    let after = &text[run.end()..];
    // A figure only spaces away from a roman run is part of that name, unless a counter after it makes it a
    // count of the thing named.
    let neighbours = [at.checked_sub(1).and_then(|before| runs.get(before)), runs.get(at + 1)];
    let beside = neighbours.into_iter().flatten().any(|next| {
      let gap = if next.start() < run.start() {
        &text[next.end()..run.start()]
      } else {
        &text[run.end()..next.start()]
      };

      is_roman(next.as_str()) && spaces_only(gap)
    });

    if beside && counter_reading(after).is_none() {
      continue;
    }

    let said = read_run(&run.as_str().chars().collect::<Vec<_>>(), after);

    out.push_str(&text[from..run.start()]);
    out.push_str(&said);
    from = run.end();

    if let Some(particle) = standing_particle(after) {
      out.push_str(&fitted_particle(&said, particle));
      from += particle.len();
    }
  }

  out.push_str(&text[from..]);
  out
}

/// A Korean stretch with every figure that stands on its own said the way it is read, and every other
/// character as written. Code is left whole for the gate, which reads it for what it names.
pub fn say_figures(text: &str) -> String {
  let mut out = String::new();
  let mut from = 0;

  for code in CODE.find_iter(text) {
    out.push_str(&say_figures_in(&text[from..code.start()]));
    out.push_str(code.as_str());
    from = code.end();
  }

  out.push_str(&say_figures_in(&text[from..]));
  out
}
