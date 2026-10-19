//! A review record as the studio reads it (docs/script-studio.md, 「Recording a line」 and 「Review state」): the
//! same rules, codes and sentences as the studio's TypeScript reading, which the import script writes with, held to
//! the stored records in `studio/fixtures/reviews.json`.

use serde_json::{Map, Number, Value, json};

/// Why a review does not read, by its code and its English sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
  BadTime,
  TimeForm,
  TimeSixty,
  TimeRange,
  TextLength,
  DuplicateLine,
  ReviewStatus,
  Span,
  RecordForm,
  /// Another window saved the review since this one was read.
  EditedElsewhere,
}

impl Fault {
  pub fn code(self) -> &'static str {
    match self {
      Self::BadTime => "bad_time",
      Self::TimeForm => "time_form",
      Self::TimeSixty => "time_sixty",
      Self::TimeRange => "time_range",
      Self::TextLength => "text_length",
      Self::DuplicateLine => "duplicate_line",
      Self::ReviewStatus => "review_status",
      Self::Span => "span",
      Self::RecordForm => "record_form",
      Self::EditedElsewhere => "edited_elsewhere",
    }
  }

  pub fn sentence(self) -> &'static str {
    match self {
      Self::BadTime => "A time is not written in a form the studio reads.",
      Self::TimeForm => "Write a time as minutes:seconds.milliseconds.",
      Self::TimeSixty => "Seconds and minutes are below 60.",
      Self::TimeRange => "A time is outside the range the studio takes.",
      Self::TextLength => "A text is longer than the studio takes, or is not text.",
      Self::DuplicateLine => "A line's id is empty or repeated.",
      Self::ReviewStatus => "A line's review state is neither pending nor done.",
      Self::Span => "Give a span's start and end together, the end after the start.",
      Self::RecordForm => "The record is not in the form the studio keeps.",
      Self::EditedElsewhere => "Another window saved this since it was read. Load it again, then save.",
    }
  }
}

impl std::fmt::Display for Fault {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(self.sentence())
  }
}

/// The emotion and the delivery a line stands with where it names none.
pub const DEFAULT_EMOTION: &str = "기본";
pub const DEFAULT_DELIVERY: &str = "보통";

/// Text lengths as the screen counts them, in UTF-16 code units.
fn length(text: &str) -> usize {
  text.encode_utf16().count()
}

/// Whitespace trimmed as the screen trims it, the byte-order mark among it.
fn trimmed(text: &str) -> &str {
  text.trim_matches(|one: char| one.is_whitespace() || one == '\u{feff}')
}

/// A number as the document writes it: whole numbers as integers, the rest as they are.
pub fn number(value: f64) -> Value {
  let value = value + 0.0;

  if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
    return Value::from(value as i64);
  }

  Number::from_f64(value).map_or(Value::Null, Value::Number)
}

// A number written as JavaScript writes one into text.
fn number_text(value: &Number) -> String {
  match value.as_f64() {
    Some(whole) if whole.fract() == 0.0 && whole.abs() < 1e21 => format!("{}", whole as i128),
    Some(fraction) => format!("{fraction}"),
    None => value.to_string(),
  }
}

/// A time in seconds from `[[h:]m:]s[.mmm]`, or none for an empty one.
pub fn seconds(value: Option<&Value>) -> Result<Option<f64>, Fault> {
  let text = match value {
    None | Some(Value::Null) => return Ok(None),
    Some(Value::String(text)) if text.is_empty() => return Ok(None),
    Some(Value::String(text)) => text.clone(),
    Some(Value::Number(written)) => number_text(written),
    Some(_) => return Err(Fault::BadTime),
  };
  let parts: Vec<&str> = trimmed(&text).split(':').collect();
  let well_formed = |part: &&str| {
    let (whole, fraction) = part.split_once('.').unwrap_or((part, ""));

    !whole.is_empty()
      && whole.chars().all(|one| one.is_ascii_digit())
      && (!part.contains('.')
        || ((1..=3).contains(&fraction.len()) && fraction.chars().all(|one| one.is_ascii_digit())))
  };

  if parts.len() > 3 || !parts.iter().all(well_formed) {
    return Err(Fault::TimeForm);
  }

  let read = |part: &str| part.parse::<f64>().unwrap_or(f64::NAN);

  if parts.len() > 1 && parts[1..].iter().any(|part| read(part) >= 60.0) {
    return Err(Fault::TimeSixty);
  }

  let total = parts.iter().fold(0.0, |sum, part| sum * 60.0 + read(part));

  if !total.is_finite() || total > 86_400.0 {
    return Err(Fault::TimeRange);
  }

  Ok(Some(total))
}

/// A review no one has written yet.
pub fn empty_review() -> Value {
  json!({ "version": 0, "appearance": "unknown", "speech": "unknown", "evidence": "", "lines": [] })
}

fn text(value: Option<&Value>, limit: usize) -> Result<String, Fault> {
  match value {
    Some(Value::String(text)) if length(text) <= limit => Ok(text.clone()),
    _ => Err(Fault::TextLength),
  }
}

// A field that may be missing, read as empty text where it is.
fn optional_text(line: &Map<String, Value>, key: &str, limit: usize) -> Result<String, Fault> {
  match line.get(key) {
    None | Some(Value::Null) => Ok(String::new()),
    given => text(given, limit),
  }
}

fn check_line(value: &Value, ids: &mut Vec<String>) -> Result<Value, Fault> {
  let empty = Map::new();
  let line = value.as_object().unwrap_or(&empty);
  let id = text(line.get("id"), 100)?;

  if id.is_empty() || ids.contains(&id) {
    return Err(Fault::DuplicateLine);
  }

  ids.push(id.clone());

  let status = line.get("reviewStatus");

  if status.is_some_and(|status| status != "pending" && status != "done") {
    return Err(Fault::ReviewStatus);
  }

  let (start, end) = (seconds(line.get("start"))?, seconds(line.get("end"))?);

  if start.is_none() != end.is_none() || matches!((start, end), (Some(start), Some(end)) if end <= start) {
    return Err(Fault::Span);
  }

  let labelled = |key: &str, otherwise: &str| -> Result<String, Fault> {
    let label = text(line.get(key), 100)?;
    let label = trimmed(&label);

    Ok(if label.is_empty() { otherwise.to_owned() } else { label.to_owned() })
  };

  let mut checked = Map::new();

  checked.insert("id".into(), json!(id));
  checked.insert("text".into(), json!(text(line.get("text"), 10_000)?));
  checked.insert("annotated".into(), json!(text(line.get("annotated"), 10_000)?));
  checked.insert("start".into(), start.map_or(Value::Null, number));
  checked.insert("end".into(), end.map_or(Value::Null, number));
  checked.insert("emotion".into(), json!(labelled("emotion", DEFAULT_EMOTION)?));
  checked.insert("delivery".into(), json!(labelled("delivery", DEFAULT_DELIVERY)?));
  checked.insert("notes".into(), json!(text(line.get("notes"), 10_000)?));
  checked.insert("file".into(), json!(optional_text(line, "file", 1_000)?));
  checked.insert("source".into(), json!(optional_text(line, "source", 2_000)?));
  checked.insert("events".into(), json!(optional_text(line, "events", 500)?));
  checked.insert("music".into(), json!(optional_text(line, "music", 100)?));
  checked.insert("overlap".into(), json!(optional_text(line, "overlap", 100)?));
  checked.insert(
    "reviewStatus".into(),
    status.cloned().filter(|status| !status.is_null()).unwrap_or(json!("pending")),
  );

  Ok(Value::Object(checked))
}

/// The review as it may be kept, or the fault saying why not.
pub fn validate_review(value: &Value) -> Result<Value, Fault> {
  let review = value.as_object().ok_or(Fault::RecordForm)?;
  let version = review
    .get("version")
    .and_then(Value::as_f64)
    .filter(|version| version.fract() == 0.0 && *version >= 0.0 && *version <= 9_007_199_254_740_991.0)
    .ok_or(Fault::RecordForm)?;
  let judged = |key: &str| {
    review
      .get(key)
      .and_then(Value::as_str)
      .filter(|said| ["unknown", "yes", "no"].contains(said))
  };
  let (Some(appearance), Some(speech)) = (judged("appearance"), judged("speech")) else {
    return Err(Fault::RecordForm);
  };
  let lines = review
    .get("lines")
    .and_then(Value::as_array)
    .filter(|lines| lines.len() <= 3_000)
    .ok_or(Fault::RecordForm)?;
  let mut ids = Vec::new();
  let mut checked = Vec::with_capacity(lines.len());

  for line in lines {
    checked.push(check_line(line, &mut ids)?);
  }

  let evidence = text(review.get("evidence"), 20_000)?;

  Ok(
    json!({ "version": number(version), "appearance": appearance, "speech": speech, "evidence": evidence, "lines": checked }),
  )
}
