//! The voice's hosted tool (docs/boundary-api.md, 「The hosted tool」): the tool host hands it one call of
//! `voice/servers`, `voice/generate` or `worker/voice_sound`, and it answers which voice nodes stand in the voice
//! node list, whether their servers answer and the voices they hold loaded, makes a sound of any words in any served
//! voice and writes it to a file, or writes a finished synthesis's sound to the file its caller names, for a session
//! the shell guard keeps off the engine's port.

mod call;
mod generate;
mod servers;
mod sound;

use serde_json::{Value, json};

pub use call::{Answer, Asked, Content, Handed, PROTOCOL, read_handed};
pub use generate::{
  Asked as GenerateAsked, ENTRY as GENERATE_ENTRY, Pace, generate, generate_at, not_spoken,
  read_generate_arguments, target, written_text,
};
pub use servers::{
  DEPLOY_SETTINGS, client_of, failure_text, listed, servers, servers_text, settings_of_process,
  settings_path,
};
pub use sound::{ENTRY as SOUND_ENTRY, not_finished, not_its_own, read_sound_arguments, sound, write};

/// The entry that reads the voice servers, by its full name.
pub const ENTRY: &str = "voice/servers";

/// The voice servers entry's arguments read: it takes none, so an empty object, or none at all.
pub fn read_arguments(arguments: &Value) -> Result<(), String> {
  match arguments {
    Value::Null => Ok(()),
    Value::Object(given) if given.is_empty() => Ok(()),
    Value::Object(given) => Err(format!(
      "{ENTRY} takes no arguments, and was given {}",
      given.keys().map(String::as_str).collect::<Vec<_>>().join(", ")
    )),
    other => Err(format!("{ENTRY} takes an empty object of arguments, not {other}")),
  }
}

/// What a request is answered with on standard output, or why it is answered with none: a kind the tool
/// does not serve, which the host takes as a failed call.
pub async fn answer(handed: &Handed) -> Result<Value, String> {
  match &handed.asked {
    Asked::Call { entry, arguments } => {
      let answered = match entry.as_str() {
        ENTRY => match read_arguments(arguments) {
          Ok(()) => match settings_of_process() {
            Ok(settings) => servers(&settings).await,
            Err(why) => Answer::error(why),
          },
          Err(why) => Answer::error(why),
        },
        GENERATE_ENTRY => match read_generate_arguments(arguments) {
          Ok(asked) => match settings_of_process() {
            Ok(settings) => generate(&settings, &asked).await,
            Err(why) => Answer::error(why),
          },
          Err(why) => Answer::error(why),
        },
        SOUND_ENTRY => match read_sound_arguments(arguments) {
          Ok(asked) => match settings_of_process() {
            Ok(settings) => sound(&settings, &asked).await,
            Err(why) => Answer::error(why),
          },
          Err(why) => Answer::error(why),
        },
        other => Answer::error(format!(
          "this tool answers {ENTRY}, {GENERATE_ENTRY} and {SOUND_ENTRY}, not {other}"
        )),
      };

      serde_json::to_value(answered).map_err(|why| why.to_string())
    }
    Asked::ResourcesList => Ok(json!({ "resources": [] })),
    Asked::ResourcesRead { .. } => Ok(json!({ "error": "this tool serves no resources" })),
    Asked::Other => Err("this tool answers a call and the resources kinds, and no other".to_owned()),
  }
}
