//! A line written for a room made into a script a voice can say (docs/voice-plan.md, 「A line is spoken as a
//! script, not as markup」).
//!
//! What a room is sent is markdown, and a voice handed markdown reads its marks aloud. Wiping the marks is
//! not enough either, since a bold title stranded above its description, or a numbered list of bare items,
//! is not how anyone says it. So the structure is unfolded into speech here — a title joined to what it
//! heads, an ordinal said as a word — and it is done mechanically, so none of the line's Korean is written
//! by a model.
//!
//! A web address is never said: it is taken out, the sentence closed round the gap, and one line at the end
//! says the text carries it.
//!
//! Three private marks travel with the script to the reading gate and are settled once it has read the
//! line: code, which the gate reads for what it names rather than letter by letter, and the particle after a
//! title, whose form turns on how the title ends once it is said.

use std::sync::LazyLock;

use bmo_text::js::{self, DOT, SPACE, is_space};
use bmo_text::web_address::{bare_address_at, is_web_address, markdown_link_at};
use regex::{Captures, Regex};

use super::words::VOICE_SCRIPT;

/// Where a piece of code opens in a script.
pub const CODE_OPEN: char = '\u{E001}';
/// Where a piece of code closes in a script.
pub const CODE_CLOSE: char = '\u{E002}';
/// Where a title's particle goes, settled once the title is said.
pub const PARTICLE_MARK: char = '\u{E000}';

// Where a token stands while emphasis is taken off around it, so a mark inside code or an address is never
// read as emphasis.
const HELD_OPEN: char = '\u{E010}';
const HELD_CLOSE: char = '\u{E011}';

// Where a web address stood in a line, until the line is closed round the gap it leaves.
const ADDRESS_MARK: char = '\u{E005}';

/// Korean letters as a regular expression's class body: the syllables and the compatibility consonants and
/// vowels.
pub(crate) const HANGUL: &str = "가-힣ㄱ-ㅎㅏ-ㅣ";

/// Whether a text holds a Korean letter: a syllable, or a compatibility consonant or vowel.
pub(crate) fn holds_korean(text: &str) -> bool {
  text
    .chars()
    .any(|character| is_syllable(character) || ('ㄱ'..='ㅣ').contains(&character))
}

/// Whether a character is a Hangul syllable.
pub(crate) fn is_syllable(character: char) -> bool {
  ('가'..='힣').contains(&character)
}

// JavaScript's line terminators, which its `.` does not match.
fn ends_line(character: char) -> bool {
  matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

// A letter, a figure or an underscore, as a word is made of.
fn is_word(character: char) -> bool {
  static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{L}\p{N}_]$").expect("a word's letter"));

  WORD.is_match(character.encode_utf8(&mut [0; 4]))
}

fn pattern(source: &str) -> Regex {
  Regex::new(source).unwrap_or_else(|error| panic!("a script's pattern {source}: {error}"))
}

static FENCE: LazyLock<Regex> = LazyLock::new(|| pattern(&format!("^[{SPACE}]*(`{{3,}}|~{{3,}})")));
static TABLE_ROW: LazyLock<Regex> =
  LazyLock::new(|| pattern(&format!(r"^[{SPACE}]*\|{DOT}*\|[{SPACE}]*$")));
static TABLE_SEPARATOR: LazyLock<Regex> =
  LazyLock::new(|| pattern(&format!(r"^[{SPACE}|:-]*-[{SPACE}|:-]*$")));
static HEADING: LazyLock<Regex> = LazyLock::new(|| {
  pattern(&format!(r"^[{SPACE}]{{0,3}}#{{1,6}}[{SPACE}]+({DOT}*?)(?:[{SPACE}]+#+)?[{SPACE}]*$"))
});
static QUOTE: LazyLock<Regex> = LazyLock::new(|| pattern(&format!(r"^[{SPACE}]*(?:>[{SPACE}]?)+")));
static LIST_ITEM: LazyLock<Regex> = LazyLock::new(|| {
  pattern(&format!(
    r"^([{SPACE}]*)(?:([-*+\x{{2022}}])|([0-9]{{1,3}})[.)])[{SPACE}]+(?:\[[ xX]\][{SPACE}]+)?({DOT}*)$"
  ))
});
// A head opening in bold, and what follows the bold on its line.
static BOLD_HEAD: LazyLock<Regex> =
  LazyLock::new(|| pattern(&format!(r"^(?:\*\*([^*]+)\*\*|__([^_]+)__)({DOT}*)$")));
// What makes that bold a title: a colon inside it, or a colon, a dash or nothing after it.
static TITLE_CLOSED: LazyLock<Regex> = LazyLock::new(|| pattern(&format!(r"[:\x{{FF1A}}][{SPACE}]*$")));
static TITLE_SEPARATOR: LazyLock<Regex> = LazyLock::new(|| {
  pattern(&format!(
    r"^[{SPACE}]*(?:[:\x{{FF1A}}][{SPACE}]*|[{SPACE}]+[\x{{2014}}\x{{2013}}-][{SPACE}]+|$)"
  ))
});
static COLON_TITLE: LazyLock<Regex> =
  LazyLock::new(|| pattern(&format!(r"^({DOT}+?)[:\x{{FF1A}}][{SPACE}]*$")));
static SENTENCE_CLOSED: LazyLock<Regex> = LazyLock::new(|| pattern(r"[.?!\x{2026}]$"));
static BREAKS: LazyLock<Regex> = LazyLock::new(|| pattern(r"\n{3,}"));

// The marks a backslash makes plain, a held token, and the address of a mail link.
static ESCAPED: LazyLock<Regex> = LazyLock::new(|| pattern(r"\\([\\`*_{}\[\]()#+\-.!|~>])"));
static HELD: LazyLock<Regex> = LazyLock::new(|| pattern(r"\x{E010}([0-9]+)\x{E011}"));
static MAIL: LazyLock<Regex> = LazyLock::new(|| pattern(&format!(r"<(mailto:[^>{SPACE}]+)>")));
static DOUBLE_MARKS: LazyLock<Regex> = LazyLock::new(|| pattern(r"\*\*|__"));

// The gaps an address leaves, closed in this order: one in brackets, one after a word naming it, one a
// Korean word is written onto, one opening the line, one closing a sentence, any other, and a colon the
// address left ending the line.
static BRACKETED_ADDRESS: LazyLock<Regex> =
  LazyLock::new(|| pattern(r"[ \t]*[(\[<\x{FF08}][ \t]*\x{E005}[ \t]*[)\]>\x{FF09}]"));
static NAMING_WORD: LazyLock<Regex> = LazyLock::new(|| {
  let words = VOICE_SCRIPT
    .spoken
    .address_words
    .iter()
    .map(|word| regex::escape(word))
    .collect::<Vec<_>>();

  pattern(&format!(r"({})[ \t]*\x{{E005}}", words.join("|")))
});
static ADDRESS: LazyLock<Regex> = LazyLock::new(|| pattern(r"\x{E005}"));
static OPENING_ADDRESS: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[ \t]*\x{E005}[ \t]*[,.;:]?[ \t]*"));
static CLOSING_ADDRESS: LazyLock<Regex> =
  LazyLock::new(|| pattern(&format!(r"([^{SPACE}])[ \t]*\x{{E005}}[ \t]*")));
static STANDING_ADDRESS: LazyLock<Regex> = LazyLock::new(|| pattern(r"[ \t]*\x{E005}[ \t]*"));
static STRANDED_COLON: LazyLock<Regex> =
  LazyLock::new(|| pattern(r"[ \t]*[:\x{FF1A}][ \t]*([.!?\x{2026}]*)$"));

// A line that speaks politely, by the endings that close its sentences.
static POLITE: LazyLock<Regex> = LazyLock::new(|| {
  let endings = VOICE_SCRIPT
    .spoken
    .polite_endings
    .iter()
    .map(|ending| regex::escape(ending))
    .collect::<Vec<_>>();

  pattern(&format!(r"(?m)(?:{})(?:[.?!\x{{2026}}~]+|$)", endings.join("|")))
});

/// The ordinal a numbered item opens with, said as a word; past the pack's ordinals, the figure with its
/// ordinal counter, for the figure rule to say.
fn ordinal(n: usize) -> String {
  let words = &VOICE_SCRIPT.spoken.ordinals;

  n.checked_sub(1)
    .and_then(|at| words.get(at))
    .cloned()
    .unwrap_or_else(|| format!("{n}{}", VOICE_SCRIPT.figures.ordinal))
}

// Every match of a pattern replaced by what `say` makes of it and of the text after it, of which it may
// take some too; a match `say` declines stays as written, and the search goes on one character later.
fn replaced_onward(
  text: &str,
  pattern: &Regex,
  mut say: impl FnMut(&Captures, &str) -> Option<(String, usize)>,
) -> String {
  let mut out = String::new();
  let mut from = 0;
  let mut at = 0;

  while let Some(found) = pattern.captures_at(text, at) {
    let whole = found.get(0).expect("a match");

    match say(&found, &text[whole.end()..]) {
      Some((said, taken)) => {
        out.push_str(&text[from..whole.start()]);
        out.push_str(&said);
        from = whole.end() + taken;
        at = from;
      }
      None => at = whole.start() + text[whole.start()..].chars().next().map_or(1, char::len_utf8),
    }
  }

  out.push_str(&text[from..]);
  out
}

// Every markdown link and picture said by its words, found by the rule the console finds links by, so an
// address holding a pair of brackets goes whole.
fn link_words(line: &[char]) -> Vec<char> {
  let mut out = vec![];
  let mut at = 0;

  while let Some(open) = line[at..].iter().position(|&character| character == '[').map(|found| at + found) {
    let Some(link) = markdown_link_at(line, open) else {
      out.extend_from_slice(&line[at..=open]);
      at = open + 1;
      continue;
    };
    let cut = if open > at && line[open - 1] == '!' { open - 1 } else { open };

    out.extend_from_slice(&line[at..cut]);
    out.extend(link.label.chars());
    at = link.end;
  }

  out.extend_from_slice(&line[at..]);
  out
}

// Every bare web address of a line swapped for the address mark, by the rule the console finds links by:
// up to the next space or Korean letter, the sentence's punctuation after it and a closing bracket it did
// not open left to the sentence.
fn marked_addresses(line: &[char]) -> Vec<char> {
  let mut out = vec![];
  let mut at = 0;

  while at < line.len() {
    let opens = matches!(line[at], 'h' | 'H') && (at == 0 || !is_word(line[at - 1]));

    match opens.then(|| bare_address_at(line, at)).flatten() {
      Some(address) => {
        out.push(ADDRESS_MARK);
        at += address.chars().count();
      }
      None => {
        out.push(line[at]);
        at += 1;
      }
    }
  }

  out
}

// A word joined to the form of a particle that fits it, where one was written after the gap.
fn fitted_onto(word: &str, particle: Option<&str>) -> String {
  format!("{word}{}", particle.map(|particle| fitted_particle(word, particle)).unwrap_or_default())
}

// The line closed round the gaps its addresses left: an address in brackets goes with them, one after a
// word naming an address goes and leaves that word the particle, one a Korean word is written onto is said
// as the word for an address, and any other goes with the space round it and a colon it leaves ending the
// line.
fn closed_gaps(words: &str, taken: &mut usize) -> String {
  let count = words.matches(ADDRESS_MARK).count();

  if count == 0 {
    return words.to_owned();
  }

  *taken += count;

  let address = &VOICE_SCRIPT.spoken.address;
  let words = BRACKETED_ADDRESS.replace_all(words, "");
  let words = replaced_onward(&words, &NAMING_WORD, |found, after| {
    let particle = standing_particle(after);

    Some((fitted_onto(&found[1], particle), particle.map_or(0, str::len)))
  });
  let words = replaced_onward(&words, &ADDRESS, |_, after| match standing_particle(after) {
    Some(particle) => Some((fitted_onto(address, Some(particle)), particle.len())),
    None => after.chars().next().is_some_and(is_syllable).then(|| (address.clone(), 0)),
  });
  let words = OPENING_ADDRESS.replace(&words, "");
  let words = replaced_onward(&words, &CLOSING_ADDRESS, |found, after| {
    let closes = after.chars().next().is_none_or(|next| ",.!?;:)]".contains(next));

    closes.then(|| (found[1].to_owned(), 0))
  });
  let words = STANDING_ADDRESS.replace_all(&words, " ");

  STRANDED_COLON.replace(&words, "$1").into_owned()
}

// A piece of inline code holding nothing but a web address.
fn is_address_code(content: &str) -> bool {
  is_web_address(content) && bare_address_at(&js::chars(content), 0).as_deref() == Some(content)
}

// Every piece of inline code held as code, or taken out as an address where it holds one alone: a run of
// backticks, the shortest content after it, and the same run closing it with no backtick after.
fn held_code(line: &[char], hold: &mut impl FnMut(String) -> String) -> String {
  let mut out = String::new();
  let mut at = 0;

  while at < line.len() {
    if line[at] == '`'
      && let Some((content, end)) = code_span(line, at)
    {
      let content = js::text(content);
      let trimmed = js::trim(&content);

      if is_address_code(trimmed) {
        out.push(ADDRESS_MARK);
      } else if !trimmed.is_empty() {
        out.push_str(&hold(format!("{CODE_OPEN}{trimmed}{CODE_CLOSE}")));
      }

      at = end;
      continue;
    }

    out.push(line[at]);
    at += 1;
  }

  out
}

// The code span opening at a backtick, as its content and the index past its closing run: the longest run
// of backticks that closes is taken first, as a backtracking reader takes it.
fn code_span(line: &[char], at: usize) -> Option<(&[char], usize)> {
  let run = line[at..].iter().take_while(|&&character| character == '`').count();

  for ticks in (1..=run).rev() {
    let start = at + ticks;

    for end in start + 1..=line.len() {
      if ends_line(line[end - 1]) {
        break;
      }

      let closes = line.len() >= end + ticks
        && line[end..end + ticks].iter().all(|&character| character == '`')
        && line.get(end + ticks) != Some(&'`');

      if closes {
        return Some((&line[start..end], end + ticks));
      }
    }
  }

  None
}

// Emphasis opened and closed by a pair of marks taken off its words: the marks, a word that does not open
// on a space, and the same marks closing it right after a character that is not a space.
fn unwrapped_pairs(text: &str, marks: &[[char; 2]]) -> String {
  let text = js::chars(text);
  let mut out = String::new();
  let mut at = 0;

  while at < text.len() {
    let mark = marks.iter().find(|mark| text[at..].starts_with(&mark[..]));

    if let Some(mark) = mark
      && let Some(end) = pair_end(&text, at + 2, |end| text[end..].starts_with(&mark[..]))
    {
      out.extend(&text[at + 2..end]);
      at = end + 2;
      continue;
    }

    out.push(text[at]);
    at += 1;
  }

  out
}

// Where the shortest emphasised word from `start` ends: it opens on no space, holds no line break, ends on
// no space, and stands right before what `closes` accepts.
fn pair_end(text: &[char], start: usize, closes: impl Fn(usize) -> bool) -> Option<usize> {
  if text.get(start).is_none_or(|&first| is_space(first)) {
    return None;
  }

  for end in start + 1..=text.len() {
    if ends_line(text[end - 1]) {
      return None;
    }

    if !is_space(text[end - 1]) && closes(end) {
      return Some(end);
    }
  }

  None
}

// Emphasis by one mark taken off its words: a mark not standing beside another of its kind, or a word's
// letter where `apart` says so, round words holding no such mark and no line break.
fn unwrapped_single(text: &str, mark: char, apart: impl Fn(char) -> bool) -> String {
  let text = js::chars(text);
  let mut out = String::new();
  let mut at = 0;

  while at < text.len() {
    if text[at] == mark
      && (at == 0 || !apart(text[at - 1]))
      && let Some(end) = single_end(&text, at + 1, mark, &apart)
    {
      out.extend(&text[at + 1..end]);
      at = end + 1;
      continue;
    }

    out.push(text[at]);
    at += 1;
  }

  out
}

fn single_end(text: &[char], start: usize, mark: char, apart: &impl Fn(char) -> bool) -> Option<usize> {
  if text.get(start).is_none_or(|&first| is_space(first)) {
    return None;
  }

  for end in start + 1..text.len() {
    let last = text[end - 1];

    if last == mark || last == '\n' {
      return None;
    }

    if !is_space(last) && text[end] == mark && text.get(end + 1).is_none_or(|&next| !apart(next)) {
      return Some(end);
    }
  }

  None
}

// The words of one line with its inline marks taken off: emphasis, strike-through and spoilers dropped, a
// link or a picture said by its words, code kept as code for the gate, and every web address taken out and
// counted.
fn spoken_inline(line: &str, taken: &mut usize) -> String {
  let mut tokens: Vec<String> = vec![];
  let mut hold = |token: String| {
    tokens.push(token);
    format!("{HELD_OPEN}{}{HELD_CLOSE}", tokens.len() - 1)
  };

  let plain = ESCAPED.replace_all(line, |found: &Captures| hold(found[1].to_owned())).into_owned();
  let coded = held_code(&js::chars(&plain), &mut hold);
  let addressed = js::text(&marked_addresses(&link_words(&js::chars(&coded))));
  let mailed = MAIL
    .replace_all(&addressed, |found: &Captures| hold(format!("{CODE_OPEN}{}{CODE_CLOSE}", &found[1])))
    .into_owned();
  let bold = unwrapped_pairs(&mailed, &[['*', '*'], ['_', '_']]);
  let struck = unwrapped_pairs(&bold, &[['~', '~']]);
  let spoiled = unwrapped_pairs(&struck, &[['|', '|']]);
  let starred = unwrapped_single(&spoiled, '*', |next| next == '*');
  let underscored = unwrapped_single(&starred, '_', is_word);
  let words = DOUBLE_MARKS.replace_all(&underscored, "");
  let closed = closed_gaps(&words, taken);
  let restored = HELD.replace_all(&closed, |found: &Captures| {
    found[1]
      .parse::<usize>()
      .ok()
      .and_then(|at| tokens.get(at))
      .cloned()
      .unwrap_or_default()
  });

  js::trim(&restored).to_owned()
}

// The one sentence closing a script its addresses were taken out of, in the line's own language and, in
// Korean, its own register: Korean unless the script holds English and no Korean.
fn attached_note(script: &str) -> &'static str {
  let words = &VOICE_SCRIPT.spoken;

  if !holds_korean(script) && script.chars().any(|character| character.is_ascii_alphabetic()) {
    return &words.attached_english;
  }

  if POLITE.is_match(script) { &words.attached_polite } else { &words.attached }
}

// A title and what it heads, as one sentence: the title takes its particle where what follows is Korean
// and the title does not already close a sentence.
fn titled(title: &str, after: &str, taken: &mut usize) -> String {
  let head = TITLE_CLOSED.replace(&spoken_inline(title, taken), "").into_owned();
  let rest = spoken_inline(after, taken);

  if rest.is_empty() {
    return head;
  }

  if SENTENCE_CLOSED.is_match(&head) {
    return format!("{head} {rest}");
  }

  let particle = if holds_korean(&rest) { PARTICLE_MARK.to_string() } else { String::new() };

  format!("{head}{particle}, {rest}")
}

// The title a head opens with in bold, and the rest of its line, or none where the bold is only emphasis at
// the start of a sentence.
fn bold_title(head: &str) -> Option<(String, String)> {
  let bold = BOLD_HEAD.captures(head)?;
  let title = bold.get(1).or(bold.get(2)).map_or("", |title| title.as_str());
  let after = bold.get(3).map_or("", |after| after.as_str());

  if !(TITLE_CLOSED.is_match(title) || TITLE_SEPARATOR.is_match(after)) {
    return None;
  }

  Some((title.to_owned(), TITLE_SEPARATOR.replace(after, "").into_owned()))
}

// One list item, or one paragraph under a bold line, as the sentence it is said as.
fn spoken_item(head: &str, rest: &[String], taken: &mut usize) -> String {
  if let Some((title, after)) = bold_title(head) {
    let joined = [vec![after], rest.to_vec()].concat().join(" ");

    return titled(&title, js::trim(&joined), taken);
  }

  if let Some(colon) = COLON_TITLE.captures(head)
    && !rest.is_empty()
  {
    return titled(&colon[1], &rest.join(" "), taken);
  }

  spoken_inline(&[vec![head.to_owned()], rest.to_vec()].concat().join(" "), taken)
}

fn is_block(line: &str) -> bool {
  js::trim(line).is_empty()
    || FENCE.is_match(line)
    || is_rule(line)
    || TABLE_ROW.is_match(line)
    || HEADING.is_match(line)
    || LIST_ITEM.is_match(line)
}

// A thematic break: up to three spaces, then three or more of one of its marks with only spaces and tabs
// between them.
fn is_rule(line: &str) -> bool {
  let lead = line.chars().take_while(|&character| is_space(character)).count();
  let mut rest = line.chars().skip(lead);

  match (lead <= 3).then(|| rest.next()).flatten() {
    Some(mark @ ('-' | '*' | '_')) => {
      let rest = rest.collect::<Vec<_>>();

      rest.iter().all(|&character| character == mark || character == ' ' || character == '\t')
        && rest.iter().filter(|&&character| character == mark).count() >= 2
    }
    _ => false,
  }
}

// The lines under the one just read that belong to it: up to a blank line or another block.
fn following(lines: &[String], at: &mut usize) -> Vec<String> {
  let mut rest = vec![];

  while *at < lines.len() && !is_block(&lines[*at]) {
    rest.push(js::trim(&lines[*at]).to_owned());
    *at += 1;
  }

  rest
}

/// The line as a script for speaking, with the private marks the reading gate and [`finished_script`]
/// settle. Lines stay apart where they were sentences of their own, since a line break is where the voice
/// takes its breath. A script its addresses were taken out of closes on the line saying they are attached.
pub fn spoken_script(text: &str) -> String {
  let text = text.replace("\r\n", "\n").replace('\r', "\n");
  let lines = text.split('\n').map(|line| QUOTE.replace(line, "").into_owned()).collect::<Vec<_>>();
  let mut out: Vec<String> = vec![];
  let mut taken = 0;
  let mut at = 0;

  while at < lines.len() {
    let line = &lines[at];
    at += 1;

    if let Some(fence) = FENCE.captures(line) {
      let opened = &fence[1];

      while at < lines.len() && !js::trim(&lines[at]).starts_with(opened) {
        at += 1;
      }

      at += 1;
      continue;
    }

    if js::trim(line).is_empty() || is_rule(line) {
      out.push(String::new());
      continue;
    }

    if TABLE_ROW.is_match(line) {
      if !TABLE_SEPARATOR.is_match(line) {
        let row = js::trim(line);
        let row = row.strip_prefix('|').unwrap_or(row);
        let row = row.strip_suffix('|').unwrap_or(row);
        let cells = row
          .split('|')
          .map(|cell| spoken_inline(cell, &mut taken))
          .filter(|cell| !cell.is_empty())
          .collect::<Vec<_>>();

        out.push(cells.join(", "));
      }

      continue;
    }

    if let Some(title) = HEADING.captures(line) {
      out.push(spoken_inline(&title[1], &mut taken));
      continue;
    }

    if let Some(item) = LIST_ITEM.captures(line) {
      let head = js::trim(&item[4]).to_owned();
      let rest = following(&lines, &mut at);
      let said = spoken_item(&head, &rest, &mut taken);

      if !said.is_empty() {
        out.push(match item.get(3) {
          Some(figure) => format!("{}, {said}", ordinal(figure.as_str().parse().unwrap_or(0))),
          None => said,
        });
      }

      continue;
    }

    let head = js::trim(line).to_owned();
    let rest = if bold_title(&head).is_none() { vec![] } else { following(&lines, &mut at) };

    out.push(spoken_item(&head, &rest, &mut taken));
  }

  let joined = out.join("\n");
  let script = js::trim(&BREAKS.replace_all(&joined, "\n\n")).to_owned();

  if taken == 0 {
    return script;
  }

  let note = attached_note(&script);

  if script.is_empty() { note.to_owned() } else { format!("{script}\n\n{note}") }
}

// A particle that changes with the sound before it, as its two forms and whether its open form also
// follows a syllable closed by the liquid final.
struct Pair {
  closed: &'static str,
  open: &'static str,
  open_after_liquid: bool,
}

static PAIRS: LazyLock<Vec<Pair>> = LazyLock::new(|| {
  let particles = &VOICE_SCRIPT.particles;
  let pairs = particles.pairs.iter().map(|pair| (pair, false));
  let liquid = particles.open_after_rieul.iter().map(|pair| (pair, true));

  pairs
    .chain(liquid)
    .map(|([closed, open], open_after_liquid)| Pair {
      closed,
      open,
      open_after_liquid,
    })
    .collect()
});

// Every form a particle that changes with the sound before it is written in, the longest first, so one
// that holds a shorter one at its start is taken whole.
static FORMS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
  let mut forms = PAIRS.iter().flat_map(|pair| [pair.closed, pair.open]).collect::<Vec<_>>();

  forms.sort_by_key(|form| std::cmp::Reverse(form.chars().count()));
  forms
});

// The final consonant index of the liquid final.
const LIQUID: u32 = 8;

// The final consonant a Hangul syllable or a figure closes on, as its index among the finals, 0 for none;
// none for any other character. A figure closes as its Sino-Korean reading does: zero, three and six on
// their own finals, one, seven and eight on the liquid one, the rest open.
fn final_consonant(character: char) -> Option<u32> {
  match character {
    '가'..='힣' => Some((u32::from(character) - u32::from('가')) % 28),
    '0' => Some(21),
    '3' => Some(16),
    '6' => Some(1),
    '1' | '7' | '8' => Some(LIQUID),
    '0'..='9' => Some(0),
    _ => None,
  }
}

// The final consonant of what the text ends on, read off its last syllable or figure past whatever else
// stands after it on the last line, 0 for none.
fn final_sound(said: &str) -> u32 {
  said
    .chars()
    .rev()
    .find(|&character| character == '\n' || is_syllable(character) || character.is_ascii_digit())
    .and_then(final_consonant)
    .unwrap_or(0)
}

// The form of a particle that fits a final consonant; a particle with no pair is handed back as it is.
fn particle_after(final_sound: u32, particle: &str) -> String {
  let Some(pair) = PAIRS.iter().find(|pair| pair.closed == particle || pair.open == particle) else {
    return particle.to_owned();
  };
  let closed = if pair.open_after_liquid {
    final_sound != 0 && final_sound != LIQUID
  } else {
    final_sound != 0
  };

  (if closed { pair.closed } else { pair.open }).to_owned()
}

/// The form of a particle that fits what it follows, read off the last syllable or figure of it: the
/// closed form after a syllable closed by a consonant, the open form after an open one.
pub fn fitted_particle(said: &str, particle: &str) -> String {
  particle_after(final_sound(said), particle)
}

/// A particle standing on its own at the start of a text: one of the forms that change with the sound
/// before them, with no Hangul syllable after it, so a word that only opens like one is not taken for it.
pub fn standing_particle(text: &str) -> Option<&'static str> {
  FORMS.iter().copied().find(|form| {
    text
      .strip_prefix(form)
      .is_some_and(|after| !after.chars().next().is_some_and(is_syllable))
  })
}

/// Each title's particle settled by how the title ends as it stands.
pub fn with_particles(script: &str) -> String {
  let particle = &VOICE_SCRIPT.spoken.title_particle;
  let mut pieces = script.split(PARTICLE_MARK);
  let mut said = pieces.next().unwrap_or_default().to_owned();

  for next in pieces {
    said = format!("{said}{}{next}", fitted_particle(&said, particle));
  }

  said
}

/// The script as the synthesiser is handed it, once the gate has read it: code marks gone and each title's
/// particle settled by how the title is said.
pub fn finished_script(script: &str) -> String {
  with_particles(&script.replace([CODE_OPEN, CODE_CLOSE], ""))
}
