//! A line's spoken words derived from its written words (`docs/boundary-api.md`, 「How the engine derives the spoken
//! words」): a model on a seat of agency's co-worker pool writes them with judgement, the agreed readings of the voice
//! name table handed to it first, and the check holds the answer to what a voice can say. An answer that fails is
//! asked again, as is every refusal and every silence on the way, with no count and no clock; the readings the
//! model chose for names the table did not hold are written into it.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::context::Context;
use crate::script::reading::{Language, VoiceName};
use crate::script::unconverted::{Unconverted, unconverted};
use crate::store::voices;
use crate::units::UnitError;
use crate::units::agency::{Agency, Seat, SeatAsk};

/// What the derivation's model is told, its system prompt.
const INSTRUCTIONS: &str = include_str!("../../prompts/spoken-words.md");

/// How long a try that did not answer, or answered words that failed, waits before it is asked again.
const ASK_AGAIN: Duration = Duration::from_secs(5);

/// How often a seat still waiting in its line, or a session still busy, is asked again.
const LOOK_AGAIN: Duration = Duration::from_secs(2);

/// The answer the model is asked for.
#[derive(Debug, Deserialize)]
struct Answer {
  spoken: String,
  #[serde(default)]
  readings: Vec<Chosen>,
}

/// A reading the model chose for a name the agreed readings did not hold.
#[derive(Debug, Deserialize)]
struct Chosen {
  written: String,
  said: String,
}

// Why one try gave no spoken words.
enum Missed {
  // The derivation's road failed on the way: agency refused or did not answer, or no agency is named.
  Road(String),
  // The model answered, and its answer did not read or failed the check.
  Answer(String),
}

impl Language {
  /// The language's name as the derivation's model is told it.
  fn named(self) -> &'static str {
    match self {
      Self::Ko => "Korean",
      Self::En => "English",
    }
  }
}

/// The spoken words of `text` said in `language`, asked until they stand; `told` is handed an English sentence
/// each time how the derivation stands changes.
pub async fn derived(
  context: &Context,
  id: i64,
  language: Language,
  text: &str,
  told: &mut (dyn FnMut(&str) + Send),
) -> String {
  let mut failed: Option<Unconverted> = None;
  let mut tries: u64 = 0;

  loop {
    tries += 1;
    told("the spoken words are being derived");

    let missed = match context.units.agency.as_ref() {
      None => {
        Missed::Road("no agency is named in the settings, so no spoken words can be derived".to_owned())
      }
      Some(agency) => match one_try(context, agency, id, tries, language, text, failed.as_ref()).await {
        Ok(answer) => match answer.and_then(|answer| held(&answer, language).map(|()| answer)) {
          Ok(answer) => {
            keep_readings(context, language, &answer.readings).await;

            return answer.spoken.trim().to_owned();
          }
          Err(Refusal::Unread(why)) => Missed::Answer(why),
          Err(Refusal::Unconverted(found)) => {
            let why = found.to_string();

            failed = Some(found);
            Missed::Answer(why)
          }
        },
        Err(why) => Missed::Road(why),
      },
    };

    match missed {
      Missed::Road(why) => told(&format!("the spoken words could not be derived: {why}; asked again")),
      Missed::Answer(why) => told(&format!("the derived spoken words did not stand: {why}; asked again")),
    }

    tokio::time::sleep(ASK_AGAIN).await;
  }
}

// Why an answer was not taken.
enum Refusal {
  Unread(String),
  Unconverted(Unconverted),
}

// Whether an answer's spoken words pass the check.
fn held(answer: &Answer, language: Language) -> Result<(), Refusal> {
  match unconverted(&answer.spoken, language) {
    Some(found) => Err(Refusal::Unconverted(found)),
    None => Ok(()),
  }
}

// The answer as the model wrote it: the one JSON object it holds, whatever text stands around it.
fn read_answer(written: &str) -> Result<Answer, Refusal> {
  let object = match (written.find('{'), written.rfind('}')) {
    (Some(start), Some(end)) if start < end => &written[start..=end],
    _ => return Err(Refusal::Unread("the answer held no JSON object".to_owned())),
  };

  serde_json::from_str(object).map_err(|why| Refusal::Unread(format!("the answer did not read: {why}")))
}

// The turn the model is asked: the line's language, the agreed readings, the written words, and the word the last
// answer failed on where it failed.
fn turn_prompt(
  language: Language,
  agreed: &[(String, String)],
  text: &str,
  failed: Option<&Unconverted>,
) -> String {
  let mut prompt = format!("The line's language: {}.\n\n", language.named());

  prompt.push_str("The agreed readings, each applied exactly as it stands:\n");

  if agreed.is_empty() {
    prompt.push_str("(none)\n");
  }

  for (written, said) in agreed {
    prompt.push_str(&format!("- {written}: {said}\n"));
  }

  prompt.push_str("\nThe written words, between the lines of three equals signs:\n===\n");
  prompt.push_str(text.trim());
  prompt.push_str("\n===\n");

  if let Some(found) = failed {
    prompt.push_str(&format!(
      "\nYour last answer failed the check: {found}. Write that word again by the rules and keep the rest.\n"
    ));
  }

  prompt.push_str("\nAnswer with the JSON object alone: `spoken` and `readings`.\n");

  prompt
}

// The agreed readings in the language: every row of the voice name table with a reading in it.
async fn agreed(context: &Context, language: Language) -> Result<Vec<(String, String)>, String> {
  let names = voices::said_names(&context.db).await.map_err(|why| why.to_string())?;

  Ok(names.into_iter().filter_map(|name| reading_of(name, language)).collect())
}

fn reading_of(name: VoiceName, language: Language) -> Option<(String, String)> {
  let said = match language {
    Language::Ko => name.ko,
    Language::En => name.en,
  };

  (!said.trim().is_empty()).then_some((name.written, said))
}

// Each reading the model chose kept in the voice name table, where its spelling stands in no row and its reading
// passes the check; a row the table holds is left as it stands.
async fn keep_readings(context: &Context, language: Language, chosen: &[Chosen]) {
  for reading in chosen {
    let written = reading.written.trim();
    let said = reading.said.trim();

    if written.is_empty() || unconverted(said, language).is_some() {
      continue;
    }

    if let Err(why) = voices::keep_reading(&context.db, written, language, said, crate::now_ms()).await {
      eprintln!("voice: the reading of {written} was not kept: {why}");
    }
  }
}

// One try on one seat: the seat asked and let in, the session started on it and its one turn asked, then the
// session ended and the seat given back however the turn went. The answer where the model gave one, else why the
// road failed.
async fn one_try(
  context: &Context,
  agency: &Agency,
  id: i64,
  tries: u64,
  language: Language,
  text: &str,
  failed: Option<&Unconverted>,
) -> Result<Result<Answer, Refusal>, String> {
  let agreed = agreed(context, language).await?;
  let spoken = &context.settings.spoken;
  let work_key = format!("role:{}", spoken.role);
  let key = format!("voice:synthesis:{id}:{tries}");
  let ask = SeatAsk {
    request_key: key.clone(),
    owner: key,
    role: spoken.role.clone(),
    selection: "auto".to_owned(),
    work_key: work_key.clone(),
    provider: spoken.provider.clone(),
  };
  let seat = agency.ask_seat(&ask).await.map_err(|why| format!("no seat was given: {why}"))?;
  let seat = match admitted(agency, seat).await {
    Ok(seat) => seat,
    Err(why) => return Err(format!("the seat was not let in: {why}")),
  };
  let start = json!({
    "agent": spoken.line,
    "scope": "apart",
    "provider": seat.provider,
    "model": seat.model,
    "cwd": "",
    "line": spoken.line,
    "systemPrompt": INSTRUCTIONS,
    "labels": { "voice": { "synthesis": id } },
    "toolsAllowed": [],
  });
  let session = match agency.start_session(&start).await {
    Ok(session) => session,
    Err(why) => {
      let _ = agency.give_back(&seat.token, "cancelled: no session", true).await;

      return Err(format!("no session was started: {why}"));
    }
  };

  if let Err(why) = agency.bind(&seat.token, &session).await {
    eprintln!("voice: synthesis {id}'s seat was not bound to its session: {why}");
  }

  let body = turn_body(&seat, id, &work_key, &turn_prompt(language, &agreed, text, failed));
  let turned = agency.turn(&session, &body).await;

  end(agency, &session).await;

  let outcome = match &turned {
    Ok(_) => "answered",
    Err(_) => "failed",
  };
  let _ = agency.give_back(&seat.token, outcome, false).await;

  match turned {
    Ok(written) => Ok(read_answer(&written)),
    Err(why) => Err(format!("the turn did not answer: {why}")),
  }
}

// A seat waited on in its line until the pool lets it in.
async fn admitted(agency: &Agency, mut seat: Seat) -> Result<Seat, UnitError> {
  while !seat.admitted() {
    if seat.ended() {
      return Err(UnitError::Refused {
        unit: "agency",
        status: 409,
        code: None,
        error: format!("seat {} was given back before it was let in", seat.token),
      });
    }

    tokio::time::sleep(LOOK_AGAIN).await;

    if let Some(now) = agency.admit(&seat.token).await? {
      seat = now;
    }
  }

  Ok(seat)
}

// The one turn's body, laid on the seat's road.
fn turn_body(seat: &Seat, id: i64, work_key: &str, prompt: &str) -> Value {
  json!({
    "prompt": prompt,
    "provider": seat.provider,
    "model": seat.model,
    "mapping": { "model": seat.model, "effort": seat.effort, "tier": seat.tier },
    "resume": { "kind": "fresh" },
    "cwd": "",
    "name": format!("voice-synthesis-{id}-spoken"),
    "title": "The spoken words of a line",
    "workKey": work_key,
    "hearsWords": false,
    "variables": {},
  })
}

// A session ended once its turn answered, a busy one asked again until it is gone; one agency would not end is left
// to agency, which ends every thrown-away session its process leaves.
async fn end(agency: &Agency, session: &str) {
  loop {
    match agency.end_session(session).await {
      Ok(true) => return,
      Ok(false) => tokio::time::sleep(LOOK_AGAIN).await,
      Err(why) => {
        eprintln!("voice: session {session} was not ended: {why}");

        return;
      }
    }
  }
}

#[doc(hidden)]
pub mod testing {
  //! The pure parts of the derivation, for the tests.

  use super::*;

  /// The turn prompt for the given agreed readings and written words, the last failure named where there was one.
  pub fn prompt(
    language: Language,
    agreed: &[(String, String)],
    text: &str,
    failed: Option<&Unconverted>,
  ) -> String {
    turn_prompt(language, agreed, text, failed)
  }

  /// The spoken words of an answer the model wrote, where it reads and passes the check, else why not.
  pub fn answer(written: &str, language: Language) -> Result<String, String> {
    let answer = read_answer(written).map_err(|why| match why {
      Refusal::Unread(why) => why,
      Refusal::Unconverted(found) => found.to_string(),
    })?;

    match held(&answer, language) {
      Ok(()) => Ok(answer.spoken),
      Err(Refusal::Unconverted(found)) => Err(found.to_string()),
      Err(Refusal::Unread(why)) => Err(why),
    }
  }
}
