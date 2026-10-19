//! The voice metadata CSV (docs/script-studio.md, 「CSV import and export」): the original template's columns, a UTF-8
//! byte-order mark, and standard quoting, so Hangul, commas and line breaks survive a spreadsheet.

use serde_json::Value;

use super::records::Asset;

pub const CSV_FIELDS: [&str; 13] = [
  "id",
  "file",
  "source",
  "start",
  "end",
  "text",
  "annotated",
  "emotion",
  "delivery",
  "events",
  "music",
  "overlap",
  "notes",
];

// How the notes column opens: the review state, done or still to do, as the reviewers read it.
const STATE_LABEL: &str = "검토 상태";
const STATE_DONE: &str = "완료";
const STATE_PENDING: &str = "필요";

/// A time as `HH:MM:SS.mmm`, or nothing for no time.
pub fn csv_time(seconds: Option<f64>) -> String {
  let Some(seconds) = seconds else {
    return String::new();
  };
  let ms = (seconds * 1000.0 + 0.5).floor();
  let hours = (ms / 3_600_000.0).floor() as u64;
  let minutes = ((ms / 60_000.0).floor() % 60.0) as u64;
  let rest = format!("{:.3}", (ms % 60_000.0) / 1000.0);

  format!("{hours:02}:{minutes:02}:{rest:0>6}")
}

fn written(value: Option<&Value>) -> String {
  match value {
    None | Some(Value::Null) => String::new(),
    Some(Value::String(text)) => text.clone(),
    Some(other) => other.to_string(),
  }
}

/// Every line of every episode given, under the template's header, each record ended by CRLF and a field quoted only
/// where it holds a quote, a comma or a line break.
pub fn studio_csv(episodes: &[(Asset, Value)]) -> Result<String, csv::Error> {
  let mut writer = csv::WriterBuilder::new()
    .terminator(csv::Terminator::CRLF)
    .quote_style(csv::QuoteStyle::Necessary)
    .from_writer(Vec::new());

  writer.write_record(CSV_FIELDS)?;

  for (asset, review) in episodes {
    for line in review["lines"].as_array().into_iter().flatten() {
      let status = if line["reviewStatus"] == "done" { STATE_DONE } else { STATE_PENDING };
      let notes = format!("{STATE_LABEL}: {status}\n{}", written(line.get("notes")));
      let source = written(line.get("source"));
      let source = if source.is_empty() { asset.path.clone() } else { source };
      let row: Vec<String> = CSV_FIELDS
        .iter()
        .map(|field| match *field {
          "notes" => notes.clone(),
          "source" => source.clone(),
          "start" | "end" => csv_time(line[*field].as_f64()),
          other => written(line.get(other)),
        })
        .collect();

      writer.write_record(&row)?;
    }
  }

  let bytes = writer.into_inner().map_err(|fault| csv::Error::from(fault.into_error()))?;

  Ok(format!("\u{feff}{}", String::from_utf8_lossy(&bytes)))
}
