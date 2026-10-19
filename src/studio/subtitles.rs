//! Automatic spans from English subtitles (docs/script-studio.md, 「Automatic spans from English subtitles」): a line
//! saved with no span is matched against the provided English track, or the automatic transcription where there is
//! none, and given the cue boundaries only where one place matches. The tracks' labels and the notes a match leaves
//! are written in Korean, for the reviewers who read them.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use scraper::{Html, Selector};
use serde_json::{Value, json};
use subtp::vtt::{VttBlock, VttTimestamp, WebVtt};

use super::review::number;

/// The automatic transcription's place among a video's tracks.
pub const TRANSCRIPT_TRACK: u64 = 1000;

/// One subtitle cue: when it stands, its words, and its place among the cues read.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
  pub start: f64,
  pub end: f64,
  pub text: String,
  pub index: usize,
}

// A cue time in seconds, summed as the screen sums `h:m:s.mmm`, so a time written back reads as written.
fn stamp(time: &VttTimestamp) -> f64 {
  let seconds: f64 = format!("{}.{:03}", time.seconds, time.milliseconds).parse().unwrap_or(f64::NAN);

  (f64::from(time.hours) * 60.0 + f64::from(time.minutes)) * 60.0 + seconds
}

// A cue's words out of its markup, each run of text standing apart.
fn cue_text(payload: &[String]) -> String {
  let fragment = Html::parse_fragment(&payload.join("\n"));
  let runs: Vec<&str> = fragment.root_element().text().collect();

  runs.join(" ").trim().to_owned()
}

/// The cues of a WebVTT document whose end follows their start, numbered from one.
pub fn subtitle_cues(text: &str) -> Vec<Cue> {
  // A document whose last cue ends the file with no line break is read as one that has it, as a player reads it.
  let ended = if text.ends_with('\n') { text.to_owned() } else { format!("{text}\n") };
  let Ok(document) = WebVtt::parse(&ended) else {
    return Vec::new();
  };
  let mut cues = Vec::new();

  for block in document.blocks {
    let VttBlock::Que(cue) = block else {
      continue;
    };
    let (start, end) = (stamp(&cue.timings.start), stamp(&cue.timings.end));

    if end > start {
      cues.push(Cue {
        start,
        end,
        text: cue_text(&cue.payload),
        index: cues.len() + 1,
      });
    }
  }

  cues
}

/// How a line's words are made comparable: the rules every line and cue is put through, the speaker label among them
/// where the settings name the spellings it is written with.
pub struct Speech {
  rules: Vec<(Regex, &'static str)>,
}

impl Speech {
  /// The rules, a cue's opening speaker label dropped where it is one of `speakers`, whatever its case.
  pub fn new(speakers: &[String]) -> Self {
    let spellings: Vec<String> = speakers
      .iter()
      .map(|one| one.trim().to_lowercase())
      .filter(|one| !one.is_empty())
      .map(|one| regex::escape(&one))
      .collect();
    let label = (!spellings.is_empty()).then(|| format!(r"^\s*(?:{})\s*:\s*", spellings.join("|")));
    let mut sources: Vec<(String, &'static str)> =
      vec![(r"<[^>]*>".into(), " "), (r"\[[^\]]*\]".into(), " ")];

    if let Some(label) = label {
      sources.push((label, ""));
    }

    sources.extend(
      [
        (r"[’']", "'"),
        (r"(?-u:\b)can't(?-u:\b)", "cannot"),
        (r"(?-u:\b)won't(?-u:\b)", "will not"),
        (r"(?-u:\b)((?-u:\w)+)n't(?-u:\b)", "$1 not"),
        (r"(?-u:\b)i'm(?-u:\b)", "i am"),
        (r"(?-u:\b)(you|we|they)'re(?-u:\b)", "$1 are"),
        (r"(?-u:\b)(i|you|we|they)'ve(?-u:\b)", "$1 have"),
        (r"'", ""),
        (r"[^\p{L}\p{N}]+", " "),
      ]
      .map(|(source, with)| (source.to_owned(), with)),
    );

    Self {
      rules: sources
        .into_iter()
        .map(|(source, with)| (Regex::new(&source).expect("a pattern of this module"), with))
        .collect(),
    }
  }

  /// A line's words as a person would say them: tags, stage directions and a speaker label gone, the common
  /// contractions spelled out, and everything but letters and digits a single space.
  pub fn words(&self, text: &str) -> String {
    let mut said = text.to_lowercase();

    for (rule, with) in &self.rules {
      said = rule.replace_all(&said, *with).into_owned();
    }

    said.trim().to_owned()
  }

  /// Where a line was said among the cues: its first and last cue and their boundaries, or none where it matches
  /// nowhere or in more than one place.
  pub fn match_subtitle(&self, text: &str, cues: &[Cue]) -> Option<Match> {
    let target = self.words(text);

    if target.is_empty() {
      return None;
    }

    let tokens: Vec<&str> = target.split(' ').collect();
    let mut candidates: Vec<(Match, f64)> = Vec::new();

    for i in 0..cues.len() {
      let mut phrase = String::new();

      for j in i..cues.len().min(i + 6) {
        if j > i && cues[j].start - cues[j - 1].end > 2.0 {
          break;
        }

        let part = self.words(&cues[j].text);

        if part.is_empty() {
          break;
        }

        if !phrase.is_empty() {
          phrase.push(' ');
        }

        phrase.push_str(&part);

        let parts: Vec<&str> = phrase.split(' ').collect();
        let edits = if phrase == target {
          0
        } else if tokens.len() >= 8 && parts.len().abs_diff(tokens.len()) <= 1 {
          distance(&tokens, &parts)
        } else {
          99
        };

        if edits <= 1 {
          let score = 1.0 - edits as f64 / tokens.len().max(parts.len()) as f64;

          candidates.push((
            Match {
              start: cues[i].start,
              end: cues[j].end,
              first: cues[i].index,
              last: cues[j].index,
            },
            score,
          ));
        }

        if parts.len() > tokens.len() + 1 {
          break;
        }
      }
    }

    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));

    let (best, score) = candidates.first()?.clone();
    let others = || candidates.iter().skip(1);

    // Nearby overlapping windows can describe the same utterance; a second location is ambiguous.
    if others().any(|(other, _)| other.end <= best.start || other.start >= best.end) {
      return None;
    }

    // Equal scoring but different cue boundaries require a person's review too.
    if others().any(|(other, other_score)| {
      *other_score == score && (other.start != best.start || other.end != best.end)
    }) {
      return None;
    }

    Some(best)
  }
}

// How many words must change to turn one run of words into the other.
fn distance(a: &[&str], b: &[&str]) -> usize {
  let mut row: Vec<usize> = (0..=b.len()).collect();

  for (i, word) in a.iter().enumerate() {
    let mut next = vec![i + 1];

    for (j, other) in b.iter().enumerate() {
      next.push((next[j] + 1).min(row[j + 1] + 1).min(row[j] + usize::from(word != other)));
    }

    row = next;
  }

  row[b.len()]
}

/// Where a line was said among the cues.
#[derive(Clone, Debug, PartialEq)]
pub struct Match {
  pub start: f64,
  pub end: f64,
  pub first: usize,
  pub last: usize,
}

fn is_asset_id(id: &str) -> bool {
  id.len() == 64 && id.chars().all(|one| matches!(one, '0'..='9' | 'a'..='f'))
}

/// One track's subtitle file, if it stands: the automatic transcription for its own place, the provided track
/// otherwise.
pub fn subtitle_file(root: &Path, id: &str, index: u64) -> Option<PathBuf> {
  if !is_asset_id(id) {
    return None;
  }

  let file = if index == TRANSCRIPT_TRACK {
    root.join("asr").join(format!("{id}.vtt"))
  } else {
    root.join("subtitles").join(format!("{id}-{index}.vtt"))
  };

  file.exists().then_some(file)
}

/// A video's subtitle tracks: those its page named, in order, and the automatic transcription where one was made.
pub fn subtitle_tracks(root: &Path, id: &str) -> Vec<Value> {
  static TRACK: OnceLock<Selector> = OnceLock::new();
  static ENGLISH: OnceLock<Regex> = OnceLock::new();

  let mut tracks = Vec::new();

  if let Ok(page) = std::fs::read_to_string(root.join("pages").join(format!("{id}.html"))) {
    let page = Html::parse_document(&page);
    let selector = TRACK.get_or_init(|| Selector::parse("track").expect("a selector"));
    let english = ENGLISH.get_or_init(|| Regex::new("(?i)English").expect("a pattern of this module"));

    for (index, track) in page.select(selector).enumerate() {
      let label = track.value().attr("label").map_or_else(|| format!("자막 {}", index + 1), str::to_owned);
      let lang = if english.is_match(&label) {
        "en"
      } else if label.contains("한국어") {
        "ko"
      } else {
        "und"
      };

      tracks.push(json!({ "index": index, "label": label, "lang": lang }));
    }
  }

  if subtitle_file(root, id, TRANSCRIPT_TRACK).is_some() {
    tracks
      .push(json!({ "index": TRANSCRIPT_TRACK, "label": "English · 자동 전사 (검토 필요)", "lang": "en" }));
  }

  tracks
}

/// The review with every line of no span given the English cue boundaries where they match it, and a note saying
/// where they came from.
pub fn align_review(speech: &Speech, root: &Path, id: &str, review: &Value) -> Value {
  let track = subtitle_tracks(root, id).into_iter().find(|track| track["lang"] == "en");
  let index = track.as_ref().and_then(|track| track["index"].as_u64());
  let cues = index
    .and_then(|index| subtitle_file(root, id, index))
    .and_then(|file| std::fs::read_to_string(file).ok())
    .map(|text| subtitle_cues(&text))
    .unwrap_or_default();
  let from = if index == Some(TRANSCRIPT_TRACK) {
    "영어 자동 전사 (검토 필요)"
  } else {
    "영어 자막"
  };
  let mut aligned = review.clone();

  for line in aligned["lines"].as_array_mut().into_iter().flatten() {
    if !line["start"].is_null() || !line["end"].is_null() {
      continue;
    }

    let Some(found) = speech.match_subtitle(line["text"].as_str().unwrap_or(""), &cues) else {
      continue;
    };

    line["start"] = number(found.start);
    line["end"] = number(found.end);

    let note = format!("{from} 기본 구간 · cue {}–{} · 발화 경계 검토 필요", found.first, found.last);
    let notes = line["notes"].as_str().unwrap_or("");

    line["notes"] = json!(if notes.is_empty() { note } else { format!("{notes}\n{note}") });
  }

  aligned
}
