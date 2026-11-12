//! The sound entry's arguments, the synthesis it may write, and the file it writes (docs/boundary-api.md, 「The
//! hosted tool」): an id above zero, the caller's own key and an absolute path; a done synthesis taken in under
//! that key; and the bytes written whole or not at all.

use std::path::PathBuf;

use bmo_voice::wire::{Synthesis, SynthesisState};
use serde_json::json;
use voice_servers_tool::{not_finished, not_its_own, read_sound_arguments, write};

fn synthesis(key: Option<&str>, state: SynthesisState) -> Synthesis {
  Synthesis {
    id: 7,
    voice_name: "alpha".to_owned(),
    text: "the words".to_owned(),
    spoken: None,
    emotion: None,
    key: key.map(str::to_owned),
    state,
    wait: None,
    waiting_on: None,
    failure: None,
    duration_ms: None,
    created_at: 0,
  }
}

// "the arguments are an id above zero, a key that holds something and an absolute path"
#[test]
fn the_arguments_are_read() {
  let asked =
    read_sound_arguments(&json!({ "synthesisId": 7, "key": "line", "path": "/tmp/line.ogg" })).unwrap();

  assert_eq!(asked.synthesis_id, 7);
  assert_eq!(asked.key, "line");
  assert_eq!(asked.path, "/tmp/line.ogg");

  for (arguments, said) in [
    (json!({}), "synthesisId"),
    (json!({ "key": "line", "path": "/tmp/line.ogg" }), "synthesisId"),
    (json!({ "synthesisId": 0, "key": "line", "path": "/tmp/line.ogg" }), "above zero"),
    (json!({ "synthesisId": 7, "key": "", "path": "/tmp/line.ogg" }), "non-blank key"),
    (json!({ "synthesisId": 7, "key": "line", "path": "line.ogg" }), "absolute path"),
    (
      json!({ "synthesisId": 7, "key": "line", "path": "/tmp/line.ogg", "voiceName": "alpha" }),
      "voiceName",
    ),
  ] {
    let refusal = read_sound_arguments(&arguments).unwrap_err();

    assert!(refusal.contains(said), "{refusal}");
  }
}

// "a synthesis with no key, or another key, is not the caller's own; the same key is"
#[test]
fn a_synthesis_is_the_callers_own_only_under_its_key() {
  assert!(
    not_its_own(&synthesis(None, SynthesisState::Done), "line")
      .unwrap()
      .contains("not taken in under a key")
  );
  assert!(
    not_its_own(&synthesis(Some("other"), SynthesisState::Done), "line")
      .unwrap()
      .contains("another key")
  );
  assert_eq!(not_its_own(&synthesis(Some("line"), SynthesisState::Done), "line"), None);
}

// "only a done synthesis has a sound, and the refusal says the state it stands in"
#[test]
fn only_a_done_synthesis_has_a_sound() {
  assert_eq!(not_finished(&synthesis(Some("line"), SynthesisState::Done)), None);

  for state in [SynthesisState::Waiting, SynthesisState::Speaking, SynthesisState::Failed] {
    let refusal = not_finished(&synthesis(Some("line"), state)).unwrap();

    assert!(refusal.contains(state.as_str()), "{refusal}");
    assert!(refusal.contains("done"), "{refusal}");
  }
}

// "the bytes are written whole, the folders named are made, and a file already there is replaced"
#[test]
fn the_sound_is_written_whole() {
  let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("voice-sound-{}", std::process::id()));
  let path = root.join("lines").join("one.ogg");
  let path = path.to_str().unwrap();

  let _ = std::fs::remove_dir_all(&root);

  write(path, b"first").unwrap();
  assert_eq!(std::fs::read(path).unwrap(), b"first");

  write(path, b"second").unwrap();
  assert_eq!(std::fs::read(path).unwrap(), b"second");

  let left: Vec<_> = std::fs::read_dir(root.join("lines"))
    .unwrap()
    .map(|entry| entry.unwrap().file_name())
    .collect();

  assert_eq!(left, [std::ffi::OsString::from("one.ogg")]);

  std::fs::remove_dir_all(&root).unwrap();
}
