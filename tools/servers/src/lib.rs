//! The voice's hosted tool (docs/boundary-api.md, 「The hosted tool」): the tool host hands it one call of
//! `voice/servers`, and it answers which voice nodes stand in the voice node list, whether their servers answer
//! and the voices they hold loaded, for a session the shell guard keeps off the engine's port.

mod call;
mod servers;

use serde_json::{Value, json};

pub use call::{Answer, Asked, Content, Handed, PROTOCOL, read_handed};
pub use servers::{
  DEPLOY_SETTINGS, failure_text, listed, servers, servers_text, settings_of_process, settings_path,
};

/// The one entry this tool answers, by its full name.
pub const ENTRY: &str = "voice/servers";

/// The entry's arguments read: it takes none, so an empty object, or none at all.
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
      let answered = if entry != ENTRY {
        Answer::error(format!("this tool answers {ENTRY}, not {entry}"))
      } else if let Err(why) = read_arguments(arguments) {
        Answer::error(why)
      } else {
        match settings_of_process() {
          Ok(settings) => servers(&settings).await,
          Err(why) => Answer::error(why),
        }
      };

      serde_json::to_value(answered).map_err(|why| why.to_string())
    }
    Asked::ResourcesList => Ok(json!({ "resources": [] })),
    Asked::ResourcesRead { .. } => Ok(json!({ "error": "this tool serves no resources" })),
    Asked::Other => Err("this tool answers a call and the resources kinds, and no other".to_owned()),
  }
}
