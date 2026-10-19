//! A line made into what a voice says (`docs/voice-plan.md`): its markup unfolded into a script, its figures said
//! by rule, its names said by the voice name table, its other roman words and figures read by the reading gate,
//! and a full reading polished for listening by a gate of its own. The gates are handed in by the caller;
//! everything here but their answers is mechanical.

pub mod figures;
pub mod polish;
pub mod reading;
pub mod spoken;
pub mod words;

/// One run of a gate as the script asks it: the material, and how the run is held. The gate's own instructions
/// are the stage's, so none is sent (bmo-gate's `docs/boundary-api.md`, 「For the voice unit: the voice gates」).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GateRequest {
  pub prompt: String,
  pub chat_id: Option<i64>,
  pub message_id: Option<i64>,
  /// The agent whose turn the gate runs in; the gate's own default outside any turn.
  pub agent: Option<String>,
  pub thinking: bool,
  pub alongside: bool,
}

impl GateRequest {
  /// A request with nothing but its material: judged for no room, outside any turn, thinking, and taking its
  /// turn among the gate's runs.
  pub fn new(prompt: impl Into<String>) -> Self {
    Self {
      prompt: prompt.into(),
      chat_id: None,
      message_id: None,
      agent: None,
      thinking: true,
      alongside: false,
    }
  }
}

/// Why a gate gave nothing to use: somebody stopped it, or it could not be run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GateError {
  Stopped(String),
  Fault(String),
}

/// Why a gate's answer was not used, as the run's log and the room's notice say it. Each has a code and an
/// English sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
  /// The gate gave no answer, or an empty one.
  NoAnswer,
  /// Somebody stopped the gate.
  Stopped,
  /// The gate could not be run, and why.
  CouldNotRun(String),
  /// No reading for a span the gate was asked about.
  NoReading { span: String },
  /// A reading left in roman letters.
  Roman { span: String, reading: String },
  /// A reading still holding a figure.
  Figure { span: String, reading: String },
  /// A script with no Korean in it, which is not polished.
  NoKorean,
  /// An answer holding one of the script's private marks.
  PrivateMarks,
  /// An answer holding a code placeholder the script did not hand it.
  UnknownPlaceholder,
  /// A code placeholder standing other than once.
  PlaceholderCount { number: usize, times: usize },
  /// A roman word or a figure the answer changed or dropped.
  Changed { word: String },
  /// A roman word or a figure the answer holds and the script did not.
  Added { word: String },
  /// An item ordinal the answer says fewer times than it has to.
  OrdinalMissing { word: String },
  /// An answer much shorter than the script, by syllables.
  TooShort { before: usize, after: usize },
  /// An answer much longer than the script, by syllables.
  TooLong { before: usize, after: usize },
}

impl Fault {
  /// The fault's code.
  pub fn code(&self) -> &'static str {
    match self {
      Self::NoAnswer => "gate_no_answer",
      Self::Stopped => "gate_stopped",
      Self::CouldNotRun(_) => "gate_not_run",
      Self::NoReading { .. } => "reading_missing",
      Self::Roman { .. } => "reading_roman",
      Self::Figure { .. } => "reading_figure",
      Self::NoKorean => "polish_no_korean",
      Self::PrivateMarks => "polish_private_marks",
      Self::UnknownPlaceholder => "polish_unknown_placeholder",
      Self::PlaceholderCount { .. } => "polish_placeholder_count",
      Self::Changed { .. } => "polish_changed",
      Self::Added { .. } => "polish_added",
      Self::OrdinalMissing { .. } => "polish_ordinal_missing",
      Self::TooShort { .. } => "polish_too_short",
      Self::TooLong { .. } => "polish_too_long",
    }
  }
}

impl std::fmt::Display for Fault {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::NoAnswer => f.write_str("the gate gave no answer"),
      Self::Stopped => f.write_str("the gate was stopped"),
      Self::CouldNotRun(why) => write!(f, "the gate could not be run: {why}"),
      Self::NoReading { span } => write!(f, "the gate gave no reading for {span}"),
      Self::Roman { span, reading } => write!(f, "the gate left roman letters: {span} -> {reading}"),
      Self::Figure { span, reading } => write!(f, "the gate left a figure unread: {span} -> {reading}"),
      Self::NoKorean => f.write_str("the script holds no Korean, so it was not polished"),
      Self::PrivateMarks => f.write_str("the answer kept one of the script's private marks"),
      Self::UnknownPlaceholder => {
        f.write_str("the answer holds a code placeholder the script did not give it")
      }
      Self::PlaceholderCount { number, times } => {
        write!(f, "code placeholder {number} stands {times} times")
      }
      Self::Changed { word } => write!(f, "a roman word or figure changed: {word}"),
      Self::Added { word } => write!(f, "a roman word or figure was added: {word}"),
      Self::OrdinalMissing { word } => write!(f, "an item ordinal went missing: {word}"),
      Self::TooShort { before, after } => {
        write!(f, "the answer is too short: {before} -> {after} syllables")
      }
      Self::TooLong { before, after } => write!(f, "the answer is too long: {before} -> {after} syllables"),
    }
  }
}

impl From<GateError> for Fault {
  fn from(error: GateError) -> Self {
    match error {
      GateError::Stopped(_) => Self::Stopped,
      GateError::Fault(why) => Self::CouldNotRun(why),
    }
  }
}
