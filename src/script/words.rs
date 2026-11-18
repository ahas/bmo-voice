//! What a line is said with once it is made a script for a voice, read from the language's data file,
//! `data/<language>/voice-script.yaml`: the words markup is unfolded into, the particles fitted to the sound
//! before them, and the words a figure is read in, each template with the `{name}` marks its file names. The file
//! is Korean by nature, recorded in `korean-ledger.json`.

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use serde::Deserialize;

/// The language the scripts are spoken in, whose data file is read.
pub const LANGUAGE: &str = "ko";

const SOURCE: &str = include_str!("../../data/ko/voice-script.yaml");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceScript {
  pub spoken: SpokenWords,
  pub particles: ParticleWords,
  pub figures: FigureWords,
}

/// The words a line's markup is unfolded into.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpokenWords {
  pub ordinals: Vec<String>,
  pub title_particle: String,
  pub address: String,
  pub address_words: Vec<String>,
  pub polite_endings: Vec<String>,
  pub sounded: Vec<[String; 2]>,
  pub attached: String,
  pub attached_polite: String,
  pub attached_english: String,
}

/// The particles that change with the sound before them, each as its closed form and its open form.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParticleWords {
  pub pairs: Vec<[String; 2]>,
  pub open_after_rieul: Vec<[String; 2]>,
}

/// The words a figure is read in.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FigureWords {
  pub digits: Vec<String>,
  pub places: Vec<String>,
  pub groups: Vec<String>,
  pub native_ones: Vec<String>,
  pub native_tens: Vec<String>,
  pub twenty: String,
  pub first: String,
  pub months: BTreeMap<String, String>,
  pub native_counters: Vec<String>,
  pub counter_ends: String,
  pub hour: String,
  pub ordinal: String,
  pub machines: String,
  pub month: String,
  pub months_counted: String,
  pub item: String,
  pub date: String,
  pub hours: String,
  pub minutes: String,
  pub seconds: String,
  pub ratio: String,
  pub fraction: String,
  pub point: String,
  pub dollars: String,
  pub won: String,
  pub plus: String,
  pub minus: String,
  pub range: String,
  pub symbols: BTreeMap<String, String>,
}

/// The script words of the language, read once.
pub static VOICE_SCRIPT: LazyLock<VoiceScript> = LazyLock::new(|| {
  serde_saphyr::from_str(SOURCE)
    .unwrap_or_else(|why| panic!("data/{LANGUAGE}/voice-script.yaml does not read: {why}"))
});

/// A template with each `{name}` mark filled in with the value of that name; a template that does not read is
/// handed back as it stands.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
  let values = values
    .iter()
    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
    .collect::<HashMap<_, _>>();

  strfmt::strfmt(template, &values).unwrap_or_else(|_| template.to_owned())
}
