//! A line's spoken words derived from its written words (`docs/boundary-api.md`, 「How the engine derives the spoken
//! words」): a model on a slot of agency's co-worker pool writes them with judgement, the agreed readings of the voice
//! name table handed to it first, and the check holds the answer to what a voice can say. An answer that fails is
//! asked again, as is every refusal and every silence on the way and every slot the pool holds back — a held slot
//! tells what holds it, and one held past its try's span fails that try — each wait longer than the one before,
//! until the policy's count of tries is spent; the readings the model chose for names the table did not hold are
//! written into it.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::context::{Clocks, Context};
use crate::script::reading::{Language, VoiceName};
use crate::script::unconverted::{Finding, found_agreed, found_in_reading};
use crate::store::voices;
use crate::units::UnitError;
use crate::units::agency::{Agency, Slot, SlotAsk, Turned};

/// What the derivation's model is told, its system prompt.
const INSTRUCTIONS: &str = include_str!("../../prompts/spoken-words.md");

/// How a derivation stands as it begins, and each time it is asked again.
pub const DERIVING: &str = "the spoken words are being derived";

/// The most characters of spoken words a line holds, a caller's and a derived one's alike (docs/boundary-api.md,
/// 「Syntheses」): the line a voice server is handed whole.
pub const MOST_SPOKEN: usize = 30_000;

/// The source label a slot is asked under, the caller's own.
const SOURCE: &str = "voice";

/// How often a slot still waiting in its line, or a session still busy, is asked again: the pace a derivation's own
/// pacing keeps by default (`crate::context::Clocks`).
pub const LOOK_AGAIN: Duration = Duration::from_secs(2);

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

/// How the tries of one line's derivation are paced (`docs/boundary-api.md`, 「The clocks」): the span of the first
/// try, doubled after each one after it up to the longest, which is both how long that try may wait in the pool's
/// line for a slot and the wait after it once it has failed, and how many tries fail before the line does.
#[derive(Clone, Copy, Debug)]
pub struct Pacing {
  /// The wait after the first try that failed.
  pub first_wait: Duration,
  /// The longest wait between two tries.
  pub longest_wait: Duration,
  /// How many tries fail before the line does.
  pub tries: u32,
  /// How often the pool is asked again whether a waiting slot has its turn.
  pub ask_again: Duration,
}

impl Pacing {
  /// The wait after the `failed`th try that failed, counted from one.
  pub fn wait_after(&self, failed: u32) -> Duration {
    let doubled = self.first_wait.saturating_mul(2_u32.saturating_pow(failed.saturating_sub(1)));

    doubled.min(self.longest_wait)
  }
}

/// The spoken words of `text` said in `language`, asked until they stand on slots asked for `asker`, or why the last
/// try failed once the pacing's tries are spent; `told` is handed an English sentence each time how the derivation
/// stands changes.
pub async fn derived(
  context: &Context,
  id: i64,
  language: Language,
  text: &str,
  asker: Option<&str>,
  told: &mut (dyn FnMut(&str) + Send),
) -> Result<String, String> {
  let pacing = context.clocks.spoken;
  let drawn = rand::random::<u32>();
  let mut failed: Option<Finding> = None;
  let mut held: Option<Held> = None;
  let mut tried = 0;

  loop {
    told(DERIVING);
    tried += 1;

    let missed = match context.units.agency.as_ref() {
      None => {
        Missed::Road("no agency is named in the settings, so no spoken words can be derived".to_owned())
      }
      Some(agency) => {
        let standing = match held.take() {
          Some(standing) => Ok(standing),
          None => {
            let mut waiting = |why: &str| {
              told(&format!("try {tried} of {} waits in the role's line for a slot: {why}", pacing.tries))
            };

            stand(context, agency, id, asker, pacing, tried, &mut waiting).await
          }
        };

        match standing {
          Err(why) => Missed::Road(why),
          Ok(standing) => {
            let turn = Try {
              name: format!("voice-synthesis-{id}-spoken-{drawn:08x}-{tried}"),
              language,
              text,
              failed: failed.as_ref(),
            };

            match tried_once(context, agency, standing, &turn).await {
              Came::Spoken(words) => return Ok(words),
              Came::Missed(missed, kept, unconverted) => {
                held = kept;

                if unconverted.is_some() {
                  failed = unconverted;
                }

                missed
              }
            }
          }
        }
      }
    };
    let why = missed.to_string();

    if tried >= pacing.tries {
      if let (Some(agency), Some(standing)) = (context.units.agency.as_ref(), held.take()) {
        release(agency, standing, "failed").await;
      }

      return Err(format!("its spoken words did not stand after {tried} tries: {why}"));
    }

    let wait = pacing.wait_after(tried);

    told(&format!("{why}; try {tried} of {}, asked again in {} s", pacing.tries, wait.as_secs()));
    context.clocks.until(context.clocks.now() + wait).await;
  }
}

// One try's turn: the run name it goes out under, which no other try carries, so a stop put to one refuses no other;
// and what its prompt hands over.
struct Try<'a> {
  name: String,
  language: Language,
  text: &'a str,
  failed: Option<&'a Finding>,
}

// What one try on a standing session came to: the spoken words, or why it missed, with the session to try again on
// where it still stands and the word a failed answer stopped at.
enum Came {
  Spoken(String),
  Missed(Missed, Option<Held>, Option<Finding>),
}

// One try on a session that stands: the words kept and the session given back once they stand, a session agency no
// longer holds, or whose turn ended with no answer, given up.
async fn tried_once(context: &Context, agency: &Agency, standing: Held, turn: &Try<'_>) -> Came {
  match one_try(context, agency, &standing, turn).await {
    // A turn that ended with no answer leaves a session nothing is read from: it is given up, and the next try
    // stands a new one.
    Ok(Tried::Ended(how)) => {
      release(agency, standing, "failed").await;

      Came::Missed(Missed::Road(how), None, None)
    }
    Ok(Tried::Answered(answer)) => match answer {
      Ok(answer) => {
        release(agency, standing, "answered").await;
        keep_readings(context, turn.language, &answer.readings).await;

        Came::Spoken(answer.spoken.trim().to_owned())
      }
      Err(Refusal::Unread(why)) => Came::Missed(Missed::Answer(why), Some(standing), None),
      Err(Refusal::Unconverted(found)) => {
        Came::Missed(Missed::Answer(found.to_string()), Some(standing), Some(found))
      }
    },
    Err(why) => {
      let missed = Missed::Road(format!("the turn did not answer: {why}"));

      // A slot or a session agency no longer holds is given up, and the next try stands a new one.
      if matches!(
        why,
        UnitError::Refused {
          status: 404 | 409 | 410,
          ..
        }
      ) {
        release(agency, standing, "failed").await;

        Came::Missed(missed, None, None)
      } else {
        Came::Missed(missed, Some(standing), None)
      }
    }
  }
}

impl std::fmt::Display for Missed {
  fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Road(why) => write!(out, "the spoken words could not be derived: {why}"),
      Self::Answer(why) => write!(out, "the derived spoken words did not stand: {why}"),
    }
  }
}

/// A slot let in and the role's session started on it, which every try of one line's derivation is a turn in.
struct Held {
  slot: Slot,
  session: String,
}

// What one try came to: the answer its turn wrote, read and held to the check, or how a turn that wrote none ended.
enum Tried {
  Answered(Result<Answer, Refusal>),
  Ended(String),
}

// Why an answer was not taken.
enum Refusal {
  Unread(String),
  Unconverted(Finding),
}

// Whether an answer's spoken words pass the check against the written words, a name the agreed readings the model was
// handed spell in Hangul said as their reading or as written, and stand within the bound every line is spoken within.
fn checked(
  answer: &Answer,
  written: &str,
  language: Language,
  agreed: &[(String, String)],
) -> Result<(), Refusal> {
  if answer.spoken.trim().chars().count() > MOST_SPOKEN {
    return Err(Refusal::Unread(format!("the spoken words run past {MOST_SPOKEN} characters")));
  }

  match found_agreed(written, &answer.spoken, language, agreed) {
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
  failed: Option<&Finding>,
) -> String {
  let mut prompt = format!("The line's language: {}.\n\n", language.named());

  prompt.push_str("The agreed readings, each applied exactly as it stands:\n");

  if agreed.is_empty() {
    prompt.push_str("(none)\n");
  }

  for (written, said) in agreed {
    prompt.push_str(&format!("- {written}: {said}\n"));
  }

  if language == Language::Ko {
    prompt.push_str("\nRuns said in one breath, each as its parts are spelled and then as it is written, the way it sounds:\n");

    for [parts, sounded] in &crate::script::words::VOICE_SCRIPT.spoken.sounded {
      prompt.push_str(&format!("- {parts}: {sounded}\n"));
    }
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

  Ok(
    names
      .into_iter()
      .filter_map(|name| reading_of(name, language))
      .filter(|(written, said)| found_in_reading(written, said, language).is_none())
      .collect(),
  )
}

fn reading_of(name: VoiceName, language: Language) -> Option<(String, String)> {
  let said = match language {
    Language::Ko => name.ko,
    Language::En => name.en,
  };

  (!said.trim().is_empty()).then_some((name.written, said))
}

// Each reading the model chose kept in the voice name table, where it is no identifier's and passes the check as the
// spoken words of its spelling: as a new row where the spelling stands in none, and over a row the check refuses;
// any other row the table holds is left as it stands.
async fn keep_readings(context: &Context, language: Language, chosen: &[Chosen]) {
  for reading in chosen {
    let written = reading.written.trim();
    let said = reading.said.trim();

    if written.is_empty() || is_identifier(written) || found_in_reading(written, said, language).is_some() {
      continue;
    }

    let kept = match voices::keep_reading(&context.db, written, language, said, crate::now_ms()).await {
      Ok(true) => Ok(()),
      Ok(false) => mend_refused(context, language, written, said).await,
      Err(why) => Err(why.to_string()),
    };

    if let Err(why) = kept {
      eprintln!("voice: the reading of {written} was not kept: {why}");
    }
  }
}

// The row the table holds for a spelling written over with a chosen reading, where the check refuses the reading the
// row holds.
async fn mend_refused(
  context: &Context,
  language: Language,
  written: &str,
  said: &str,
) -> Result<(), String> {
  let held = voices::said_name(&context.db, written).await.map_err(|why| why.to_string())?;
  let Some((spelling, refused)) = held.and_then(|row| reading_of(row, language)) else {
    return Ok(());
  };

  if found_in_reading(&spelling, &refused, language).is_none() {
    return Ok(());
  }

  voices::mend_reading(&context.db, &spelling, language, &refused, said, crate::now_ms())
    .await
    .map(|_| ())
    .map_err(|why| why.to_string())
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

// A slot asked under the synthesis's owner and a key drawn for it, for the line's asker, let in within the `pacing`'s
// span for its `tried` try — what the line holds it for told as it stands — and a role's session started on it, which
// agency binds the slot to; a slot whose session would not start is given back.
async fn stand(
  context: &Context,
  agency: &Agency,
  id: i64,
  asker: Option<&str>,
  pacing: Pacing,
  tried: u32,
  told: &mut (dyn FnMut(&str) + Send),
) -> Result<Held, String> {
  let spoken = &context.settings.spoken;
  let owner = owner(id);
  let ask = SlotAsk {
    request_key: format!("{owner}:{:016x}", rand::random::<u64>()),
    owner,
    role: spoken.role.clone(),
    source: SOURCE.to_owned(),
    asked_by: asker.map(str::to_owned),
    provider: spoken.provider.clone(),
    model: spoken.model.clone(),
  };
  let slot = agency.ask_slot(&ask).await.map_err(|why| format!("no slot was given: {why}"))?;
  let token = slot.token.clone();
  let slot = match admitted(agency, &context.clocks, slot, pacing, tried, told).await {
    Ok(slot) => slot,
    Err(why) => {
      give_back(agency, &token, "cancelled: not let in", true).await;

      return Err(why);
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

/// Everything one synthesis holds of agency given back by its owner, as a derivation ended under its try leaves it.
pub async fn give_back_owned(agency: &Agency, id: i64) {
  let owner = owner(id);

  if let Err(why) = agency.release_owner(&owner).await {
    eprintln!("voice: what {owner} held of agency was not given back: {why}");
  }
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
  turn: &Try<'_>,
) -> Result<Tried, UnitError> {
  let agreed = agreed(context, turn.language).await.map_err(|why| UnitError::Unreached {
    unit: "the voice's store",
    why,
  })?;
  let body = turn_body(&turn.name, &turn_prompt(turn.language, &agreed, turn.text, turn.failed));

  Ok(match agency.turn(&held.session, &body).await? {
    Turned::Wrote(written) => Tried::Answered(
      read_answer(&written)
        .and_then(|answer| checked(&answer, turn.text, turn.language, &agreed).map(|()| answer)),
    ),
    ended @ Turned::Ended { .. } => Tried::Ended(ended.to_string()),
  })
}

// A slot waited on in its role's line: its admission asked again on the pacing's own while the pool holds it back,
// since the line lets in only a request whose owner is asking now, and what the pool holds it for told each time it
// changes. A slot the pool gives back before it is let in is asked about no more; one not let in within the span its
// try is waited for gives that try up, naming what held it.
async fn admitted(
  agency: &Agency,
  clocks: &Clocks,
  mut slot: Slot,
  pacing: Pacing,
  tried: u32,
  told: &mut (dyn FnMut(&str) + Send),
) -> Result<Slot, String> {
  // A try may stand in the role's line for as long as the line waits after it, so the pacing's tries bound a road
  // that cannot carry the work as surely as they bound an answer that will not stand.
  let span = pacing.wait_after(tried);
  let due = clocks.now() + span;
  let mut said = String::new();

  while !slot.admitted() {
    if slot.ended() {
      let by = slot.ended_by.as_deref().unwrap_or("agency");
      let why = slot.outcome.as_deref().map(|outcome| format!(": {outcome}")).unwrap_or_default();

      return Err(format!("slot {} was {} by {by} before it was let in{why}", slot.token, slot.state));
    }

    let held = held_for(agency, &slot.token).await;

    if held != said {
      told(&held);

      said = held;
    }

    clocks.until(clocks.now() + pacing.ask_again).await;

    // Agency not reached is asked again about the same slot, which keeps its place in the line.
    match agency.admit(&slot.token).await {
      Ok(now) => slot = now,
      Err(
        UnitError::Unreached { .. }
        | UnitError::Refused {
          status: 502 | 503, ..
        },
      ) => {}
      Err(why) => return Err(why.to_string()),
    }

    // A try's span is spent only once its line has been asked again and stands there still.
    if !slot.admitted() && clocks.now() >= due {
      return Err(format!("the pool did not let it in within {} s: {said}", span.as_secs()));
    }
  }

  Ok(slot)
}

// What the pool holds a waiting slot for, in its own words kept on one line, or that it has said no reason.
async fn held_for(agency: &Agency, token: &str) -> String {
  match agency.held(token).await {
    Ok(request) => match request.held_for.as_deref().map(str::trim) {
      Some(held) if !held.is_empty() => held.lines().collect::<Vec<_>>().join("; "),
      _ => "the pool has given no reason for holding the slot".to_owned(),
    },
    Err(why) => format!("the held slot could not be read: {why}"),
  }
}

// The one turn's body: the turn's own parts alone, its road the session's.
fn turn_body(name: &str, prompt: &str) -> Value {
  json!({
    "prompt": prompt,
    "resume": { "kind": "fresh" },
    "name": name,
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
    failed: Option<&Finding>,
  ) -> String {
    turn_prompt(language, agreed, text, failed)
  }

  /// The spoken words of an answer the model wrote for the written words `text`, where it reads and passes the
  /// check, else why not.
  pub fn answer(written: &str, text: &str, language: Language) -> Result<String, String> {
    answer_agreed(written, text, language, &[])
  }

  /// The spoken words of an answer as `answer` reads them, the model having been handed the agreed readings
  /// `agreed`.
  pub fn answer_agreed(
    written: &str,
    text: &str,
    language: Language,
    agreed: &[(String, String)],
  ) -> Result<String, String> {
    let answer = read_answer(written).map_err(|why| match why {
      Refusal::Unread(why) => why,
      Refusal::Unconverted(found) => found.to_string(),
    })?;

    match checked(&answer, text, language, agreed) {
      Ok(()) => Ok(answer.spoken),
      Err(Refusal::Unconverted(found)) => Err(found.to_string()),
      Err(Refusal::Unread(why)) => Err(why),
    }
  }
}
