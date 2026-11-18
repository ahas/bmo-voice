//! A line's spoken words derived from its written words (`docs/boundary-api.md`, 「How the engine derives the spoken
//! words」): a model on a slot of agency's co-worker pool writes them with judgement, the agreed readings of the voice
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
use crate::units::agency::{Agency, Slot, SlotAsk};

/// What the derivation's model is told, its system prompt.
const INSTRUCTIONS: &str = include_str!("../../prompts/spoken-words.md");

/// How a derivation stands as it begins, and each time it is asked again.
pub const DERIVING: &str = "the spoken words are being derived";

/// The most characters of spoken words a line holds, a caller's and a derived one's alike (docs/boundary-api.md,
/// 「Syntheses」): the line a voice server is handed whole.
pub const MOST_SPOKEN: usize = 30_000;

/// How long a try that did not answer, or answered words that failed, waits before it is asked again.
const ASK_AGAIN: Duration = Duration::from_secs(5);

/// The source label a slot is asked under, the caller's own.
const SOURCE: &str = "voice";

/// How often a slot still waiting in its line, or a session still busy, is asked again.
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
  let mut held: Option<Held> = None;

  loop {
    told(DERIVING);

    let missed = match context.units.agency.as_ref() {
      None => {
        Missed::Road("no agency is named in the settings, so no spoken words can be derived".to_owned())
      }
      Some(agency) => {
        if held.is_none() {
          match stand(context, agency, id).await {
            Ok(standing) => held = Some(standing),
            Err(why) => {
              told(&format!("the spoken words could not be derived: {why}; asked again"));
              tokio::time::sleep(ASK_AGAIN).await;
              continue;
            }
          }
        }

        let Some(standing) = held.as_ref() else {
          continue;
        };

        match one_try(context, agency, standing, id, language, text, failed.as_ref()).await {
          Ok(answer) => match answer.and_then(|answer| checked(&answer, language).map(|()| answer)) {
            Ok(answer) => {
              if let Some(standing) = held.take() {
                release(agency, standing, "answered").await;
              }

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
          Err(why) => {
            // A slot or a session agency no longer holds is given up, and the next try stands a new one.
            if matches!(
              why,
              UnitError::Refused {
                status: 404 | 409 | 410,
                ..
              }
            ) && let Some(standing) = held.take()
            {
              release(agency, standing, "failed").await;
            }

            Missed::Road(format!("the turn did not answer: {why}"))
          }
        }
      }
    };

    match missed {
      Missed::Road(why) => told(&format!("the spoken words could not be derived: {why}; asked again")),
      Missed::Answer(why) => told(&format!("the derived spoken words did not stand: {why}; asked again")),
    }

    tokio::time::sleep(ASK_AGAIN).await;
  }
}

/// A slot let in and the role's session started on it, which every try of one line's derivation is a turn in.
struct Held {
  slot: Slot,
  session: String,
}

// Why an answer was not taken.
enum Refusal {
  Unread(String),
  Unconverted(Unconverted),
}

// Whether an answer's spoken words pass the check and stand within the bound every line is spoken within.
fn checked(answer: &Answer, language: Language) -> Result<(), Refusal> {
  if answer.spoken.trim().chars().count() > MOST_SPOKEN {
    return Err(Refusal::Unread(format!("the spoken words run past {MOST_SPOKEN} characters")));
  }

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

// Each reading the model chose kept in the voice name table, where its spelling stands in no row, is no identifier
// and its reading passes the check; a row the table holds is left as it stands.
async fn keep_readings(context: &Context, language: Language, chosen: &[Chosen]) {
  for reading in chosen {
    let written = reading.written.trim();
    let said = reading.said.trim();

    if written.is_empty() || is_identifier(written) || unconverted(said, language).is_some() {
      continue;
    }

    if let Err(why) = voices::keep_reading(&context.db, written, language, said, crate::now_ms()).await {
      eprintln!("voice: the reading of {written} was not kept: {why}");
    }
  }
}

// The shortest run of hexadecimal characters taken for an identifier: a commit hash as git abbreviates it.
const IDENTIFIER_LEAST: usize = 7;

// Whether a spelling is an identifier rather than a name: a run of hexadecimal characters as long as an abbreviated
// commit hash, holding a digit. It names one commit or one digest, so a reading of it would never be read again.
fn is_identifier(written: &str) -> bool {
  written.len() >= IDENTIFIER_LEAST
    && written.chars().all(|letter| letter.is_ascii_hexdigit())
    && written.chars().any(|letter| letter.is_ascii_digit())
}

// A slot asked under the synthesis's owner and a key drawn for it, let in, and a role's session started on it,
// which agency binds the slot to; a slot whose session would not start is given back.
async fn stand(context: &Context, agency: &Agency, id: i64) -> Result<Held, String> {
  let spoken = &context.settings.spoken;
  let owner = owner(id);
  let ask = SlotAsk {
    request_key: format!("{owner}:{:016x}", rand::random::<u64>()),
    owner,
    role: spoken.role.clone(),
    source: SOURCE.to_owned(),
    provider: spoken.provider.clone(),
    model: spoken.model.clone(),
  };
  let slot = agency.ask_slot(&ask).await.map_err(|why| format!("no slot was given: {why}"))?;
  let token = slot.token.clone();
  let slot = match admitted(agency, slot).await {
    Ok(slot) => slot,
    Err(why) => {
      give_back(agency, &token, "cancelled: not let in", true).await;

      return Err(format!("the slot was not let in: {why}"));
    }
  };
  let start = json!({
    "scope": "role",
    "slot": slot.token,
    "line": spoken.line,
    "systemPrompt": INSTRUCTIONS,
    "labels": { "voice": { "synthesis": id } },
    "toolsAllowed": [],
  });
  let session = match agency.start_session(&start).await {
    Ok(session) => session,
    Err(why) => {
      give_back(agency, &slot.token, "cancelled: no session", true).await;

      return Err(format!("no session was started: {why}"));
    }
  };

  Ok(Held { slot, session })
}

// A derivation's session ended, which gives back the slot agency bound to it, and the slot then finished with what
// it came to.
async fn release(agency: &Agency, held: Held, outcome: &str) {
  end(agency, &held.session).await;
  give_back(agency, &held.slot.token, outcome, false).await;
}

// A slot given back; one agency did not take back is left to the owner's release as the next process starts.
async fn give_back(agency: &Agency, token: &str, outcome: &str, cancel: bool) {
  match agency.give_back(token, outcome, cancel).await {
    Ok(()) | Err(UnitError::Refused { status: 404, .. }) => {}
    Err(why) => eprintln!("voice: the slot {token} was not given back: {why}"),
  }
}

/// The owner a synthesis's slots are asked under.
fn owner(id: i64) -> String {
  format!("voice:synthesis:{id}")
}

/// Everything the syntheses an earlier process left unspoken hold of agency given back by their owners, before
/// any of them is taken up (docs/boundary-api.md, 「How the engine derives the spoken words」); how many slots
/// were.
pub async fn give_back_left(agency: &Agency, db: &sea_orm::DatabaseConnection) -> usize {
  let left = match crate::store::syntheses::unfinished(db).await {
    Ok(left) => left,
    Err(why) => {
      eprintln!("voice: the syntheses an earlier process left were not read: {why}");

      return 0;
    }
  };
  let mut given = 0;

  for row in left {
    let owner = owner(row.id);

    match agency.release_owner(&owner).await {
      Ok(released) => given += (released.cancelled + released.released) as usize,
      Err(why) => eprintln!("voice: what {owner} held of agency was not given back: {why}"),
    }
  }

  given
}

// One try: one turn in the derivation's session. The answer where the model gave one, else agency's refusal or
// silence.
async fn one_try(
  context: &Context,
  agency: &Agency,
  held: &Held,
  id: i64,
  language: Language,
  text: &str,
  failed: Option<&Unconverted>,
) -> Result<Result<Answer, Refusal>, UnitError> {
  let agreed = agreed(context, language).await.map_err(|why| UnitError::Unreached {
    unit: "the voice's store",
    why,
  })?;
  let body = turn_body(id, &turn_prompt(language, &agreed, text, failed));
  let written = agency.turn(&held.session, &body).await?;

  Ok(read_answer(&written))
}

// A slot waited on in its role's line, its admission asked again and again until the pool lets it in: the line lets
// in only a request whose owner is asking now. One the admission answers given back is asked about no more.
async fn admitted(agency: &Agency, mut slot: Slot) -> Result<Slot, UnitError> {
  while !slot.admitted() {
    if slot.ended() {
      let by = slot.ended_by.as_deref().unwrap_or("agency");
      let why = slot.outcome.as_deref().map(|outcome| format!(": {outcome}")).unwrap_or_default();

      return Err(UnitError::Refused {
        unit: "agency",
        status: 409,
        code: None,
        error: format!("slot {} was {} by {by} before it was let in{why}", slot.token, slot.state),
      });
    }

    tokio::time::sleep(LOOK_AGAIN).await;

    // Agency not reached is asked again about the same slot, which keeps its place in the line.
    match agency.admit(&slot.token).await {
      Ok(now) => slot = now,
      Err(
        UnitError::Unreached { .. }
        | UnitError::Refused {
          status: 502 | 503, ..
        },
      ) => {}
      Err(why) => return Err(why),
    }
  }

  Ok(slot)
}

// The one turn's body: the turn's own parts alone, its road the session's.
fn turn_body(id: i64, prompt: &str) -> Value {
  json!({
    "prompt": prompt,
    "resume": { "kind": "fresh" },
    "name": format!("voice-synthesis-{id}-spoken"),
    "title": "The spoken words of a line",
    // The derivation stands under no room.
    "chat": 0,
    "hearsWords": false,
    "variables": {},
  })
}

// A session ended once the spoken words stand, a busy one asked again until it is gone; one agency would not end is left
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

    match checked(&answer, language) {
      Ok(()) => Ok(answer.spoken),
      Err(Refusal::Unconverted(found)) => Err(found.to_string()),
      Err(Refusal::Unread(why)) => Err(why),
    }
  }
}
