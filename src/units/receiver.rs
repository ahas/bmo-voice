//! bmo-receiver, as its `docs/boundary-api.md` states it: a notice the room's voice says, the voice briefing's
//! switch, a room's answerers, the owner, a message, a voice message put in a room, and the event stream. The
//! routes of bmo-job's family take snake_case bodies; the receiver's own take camelCase.

use std::path::Path;

use reqwest::Method;
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};

use super::{Caller, UnitError};

/// A notice for a room's voice to say in its own words (the receiver's `POST /rooms/{chat}/notices`): the fact,
/// what the line is for and what follows it, whether it may go unsaid, the kind a newer notice replaces, how long
/// it may wait, the events its delivery carries, and how long it rides the next turn rather than opening one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notice {
  pub fact: String,
  pub intent: String,
  pub follow_up: Option<String>,
  pub optional: bool,
  pub kind: Option<String>,
  pub ttl_ms: Option<i64>,
  pub events: Vec<String>,
  pub rides_ms: Option<i64>,
}

#[derive(Serialize)]
struct NoticeBody<'a> {
  fact: &'a str,
  intent: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  follow_up: Option<&'a str>,
  optional: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  kind: Option<&'a str>,
  #[serde(skip_serializing_if = "Option::is_none")]
  expires_at: Option<i64>,
  events: &'a [String],
  #[serde(skip_serializing_if = "Option::is_none")]
  rides_until: Option<i64>,
}

#[derive(Deserialize)]
struct Queued {
  id: serde_json::Value,
}

/// The voice briefing's two switches as they stand.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub struct Switch {
  pub on: bool,
  pub autoplay: bool,
}

/// One agent named to answer in a room.
#[derive(Clone, Debug, Deserialize)]
pub struct Answerer {
  pub agent: String,
  #[serde(default)]
  pub companion: bool,
}

/// A room's answerers: the agent that answers where it names none, the agent that speaks for it, and every agent
/// named to answer that is not resting.
#[derive(Clone, Debug, Deserialize)]
pub struct Answerers {
  pub default: String,
  pub voice: String,
  #[serde(default)]
  pub answerers: Vec<Answerer>,
}

/// The owner as a sender, and the owner's private chat.
#[derive(Clone, Debug, Deserialize)]
pub struct Owner {
  pub sender: i64,
  pub chat: i64,
}

/// A message the receiver keeps.
#[derive(Clone, Debug, Deserialize)]
pub struct Message {
  pub sender: Option<i64>,
  pub sender_name: Option<String>,
  pub sent_at: i64,
  pub text: String,
}

/// One file of a send and the message it went out as.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
  pub name: String,
  pub message_id: i64,
}

/// One file a send did not take, and why.
#[derive(Clone, Debug, Deserialize)]
pub struct Failed {
  pub name: String,
  pub reason: String,
}

/// What a send with files answered.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentFiles {
  pub message_id: Option<i64>,
  #[serde(default)]
  pub sent: Vec<Sent>,
  #[serde(default)]
  pub failed: Vec<Failed>,
}

/// The receiver.
#[derive(Clone, Debug)]
pub struct Receiver {
  caller: Caller,
}

impl Receiver {
  pub fn new(caller: Caller) -> Self {
    Self { caller }
  }

  /// A notice queued for the room's voice, `now` the moment its lapse and ride are counted from; the instruction
  /// it was queued as.
  pub async fn notice(&self, chat: i64, notice: &Notice, now: i64) -> Result<String, UnitError> {
    let body = NoticeBody {
      fact: &notice.fact,
      intent: &notice.intent,
      follow_up: notice.follow_up.as_deref(),
      optional: notice.optional,
      kind: notice.kind.as_deref(),
      expires_at: notice.ttl_ms.map(|ttl| now + ttl),
      events: &notice.events,
      rides_until: notice.rides_ms.map(|rides| now + rides),
    };
    let sent = self.caller.request(Method::POST, &["rooms", &chat.to_string(), "notices"]).json(&body);
    let queued: Queued = self.caller.json(sent).await?;

    Ok(match queued.id {
      serde_json::Value::String(id) => id,
      other => other.to_string(),
    })
  }

  /// How the voice briefing's switches stand.
  pub async fn switch(&self) -> Result<Switch, UnitError> {
    self.caller.json(self.caller.request(Method::GET, &["voice-briefing"])).await
  }

  /// A room's answerers.
  pub async fn answerers(&self, chat: i64) -> Result<Answerers, UnitError> {
    let sent = self.caller.request(Method::GET, &["rooms", &chat.to_string(), "answerers"]);

    self.caller.json(sent).await
  }

  /// The owner and the owner's private chat.
  pub async fn owner(&self) -> Result<Owner, UnitError> {
    self.caller.json(self.caller.request(Method::GET, &["owner"])).await
  }

  /// One message of a room; none where the receiver keeps no such message.
  pub async fn message(&self, chat: i64, message: i64) -> Result<Option<Message>, UnitError> {
    let sent = self
      .caller
      .request(Method::GET, &["rooms", &chat.to_string(), "messages", &message.to_string()]);

    match self.caller.json(sent).await {
      Ok(found) => Ok(Some(found)),
      Err(UnitError::Refused { status: 404, .. }) => Ok(None),
      Err(why) => Err(why),
    }
  }

  /// A sound file put in a room with its caption, in reply to a message where one is named.
  pub async fn send_file(
    &self,
    chat: i64,
    file: &Path,
    caption: Option<&str>,
    reply_to: Option<i64>,
  ) -> Result<SentFiles, UnitError> {
    let unreached = |why: std::io::Error| UnitError::Unreached {
      unit: "receiver",
      why: why.to_string(),
    };
    let bytes = tokio::fs::read(file).await.map_err(unreached)?;
    let name = file
      .file_name()
      .map_or_else(|| "voice.ogg".to_owned(), |name| name.to_string_lossy().into_owned());
    let part =
      Part::bytes(bytes)
        .file_name(name)
        .mime_str("audio/ogg")
        .map_err(|why| UnitError::Unreached {
          unit: "receiver",
          why: why.to_string(),
        })?;
    let mut form = Form::new().text("chatId", chat.to_string()).part("file", part);

    if let Some(caption) = caption {
      form = form.text("text", caption.to_owned());
    }

    if let Some(reply_to) = reply_to {
      form = form.text("replyTo", reply_to.to_string());
    }

    self.caller.json(self.caller.request(Method::POST, &["say-file"]).multipart(form)).await
  }

  /// The receiver's event stream opened, its answer handed back for the reader to take its frames from.
  pub async fn events(&self) -> Result<reqwest::Response, UnitError> {
    self.caller.send(self.caller.request(Method::GET, &["events"])).await
  }

  /// A room's name, as the room list gives it.
  pub async fn room_title(&self, chat: i64) -> Result<String, UnitError> {
    #[derive(Deserialize)]
    struct Titled {
      title: String,
    }

    let sent = self.caller.request(Method::GET, &["console", "rooms", &chat.to_string(), "title"]);
    let titled: Titled = self.caller.json(sent).await?;

    Ok(titled.title)
  }

  /// News that a message of a room changed, put into the receiver's stream so a screen showing the room reads it
  /// again.
  pub async fn message_changed(&self, chat: i64, message: i64) -> Result<(), UnitError> {
    let body = serde_json::json!({ "type": "message", "chatId": chat, "messageId": message });

    self
      .caller
      .send(self.caller.request(Method::POST, &["event"]).json(&body))
      .await
      .map(drop)
  }
}
