//! The tool's manifest and input schemas held to the executable (bmo-pocket's `docs/api.md`, 「The manifest」;
//! docs/boundary-api.md, 「The hosted tool」): the tool named for its folder, its executable the crate's own
//! binary, its two entries the ones the executable answers, each entry's schema, and the deployment placing what
//! the manifest names.

use std::path::{Path, PathBuf};

use serde_json::Value;
use voice_servers_tool::{ENTRY, PROTOCOL, SOUND_ENTRY};

fn folder() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn toml_of(path: &Path) -> toml::Value {
  toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn text<'a>(value: &'a toml::Value, path: &[&str]) -> &'a str {
  path.iter().fold(value, |at, key| &at[*key]).as_str().unwrap()
}

fn words(value: &toml::Value) -> Vec<&str> {
  value.as_array().unwrap().iter().map(|word| word.as_str().unwrap()).collect()
}

fn entry<'a>(manifest: &'a toml::Value, full_name: &str) -> &'a toml::Value {
  manifest["entry"]
    .as_array()
    .unwrap()
    .iter()
    .find(|entry| format!("{}/{}", text(entry, &["namespace"]), text(entry, &["name"])) == full_name)
    .unwrap_or_else(|| panic!("the manifest declares {full_name}"))
}

// "the manifest names the tool for its folder and the crate's binary, with no span declared for a call"
#[test]
fn the_manifest_names_the_tool_for_its_folder_and_runs_the_crates_binary() {
  let manifest = toml_of(&folder().join("tool.toml"));
  let crate_manifest = toml_of(&folder().join("Cargo.toml"));
  let package = text(&crate_manifest, &["package", "name"]);
  let binary = text(&crate_manifest["bin"][0], &["name"]);

  assert_eq!(text(&manifest, &["tool", "name"]), folder().file_name().unwrap().to_str().unwrap());
  assert_eq!(manifest["tool"]["protocol"].as_integer(), Some(i64::from(PROTOCOL)));
  assert_eq!(text(&manifest, &["tool", "language"]), "rust");
  assert_eq!(words(&manifest["tool"]["run"]), [binary]);
  assert!(text(&manifest, &["tool", "build"]).contains(&format!("-p {package} ")));
  assert!(text(&manifest, &["tool", "build"]).contains(&format!("/release/{binary}\"")));
  assert_eq!(text(&manifest, &["tool", "check"]), format!("bmo-testbed cargo test -p {package}"));
  assert!(manifest["tool"].get("timeout").is_none());
}

// "its two entries are the ones the executable answers: the servers read-only under room.tools, and the sound a
// worker reaches under room.send"
#[test]
fn its_entries_are_the_ones_the_executable_answers() {
  let manifest = toml_of(&folder().join("tool.toml"));
  let entries = manifest["entry"].as_array().unwrap();

  assert_eq!(entries.len(), 2, "{entries:?}");

  let servers = entry(&manifest, ENTRY);
  assert_eq!(words(&servers["capabilities"]), ["room.tools"]);
  assert!(servers.get("grant").is_none());
  assert!(servers.get("system").is_none());
  assert!(servers.get("timeout").is_none());
  assert!(servers.get("unbounded").is_none());
  assert_eq!(servers["hints"]["read_only"].as_bool(), Some(true));
  assert!(!text(servers, &["description"]).is_empty());

  let sound = entry(&manifest, SOUND_ENTRY);
  assert_eq!(text(sound, &["namespace"]), "worker");
  assert_eq!(words(&sound["capabilities"]), ["room.send"]);
  assert!(sound.get("grant").is_none());
  assert!(sound.get("system").is_none());
  assert!(sound.get("timeout").is_none());
  assert!(sound.get("unbounded").is_none());
  assert!(sound.get("hints").unwrap().get("read_only").is_none());
  assert!(!text(sound, &["description"]).is_empty());

  assert!(manifest.get("resources").is_none());
}

// "the servers schema is an empty object and the sound schema takes the id, the key and the absolute path"
#[test]
fn each_entrys_schema_is_the_one_it_takes() {
  let manifest = toml_of(&folder().join("tool.toml"));
  let schema_of = |full_name: &str| -> (String, Value) {
    let input = text(entry(&manifest, full_name), &["input"]);
    let schema = serde_json::from_str(&std::fs::read_to_string(folder().join(input)).unwrap()).unwrap();

    (input.to_owned(), schema)
  };

  let (servers_input, servers) = schema_of(ENTRY);
  assert_eq!(servers_input, "servers.json");
  assert_eq!(servers["type"], "object");
  assert_eq!(servers["additionalProperties"], false);
  assert!(servers["properties"].as_object().unwrap().is_empty());
  assert!(servers.get("required").is_none());

  let (sound_input, sound) = schema_of(SOUND_ENTRY);
  assert_eq!(sound_input, "voice_sound.json");
  assert_eq!(sound["type"], "object");
  assert_eq!(sound["additionalProperties"], false);
  assert_eq!(sound["required"], serde_json::json!(["synthesisId", "key", "path"]));
  assert_eq!(sound["properties"]["synthesisId"]["type"], "integer");
  assert_eq!(sound["properties"]["key"]["type"], "string");
  assert_eq!(sound["properties"]["path"]["type"], "string");
}

// "the workspace lists the crate, the release builds its binary, and the deployment places the binary, the
// manifest and every entry's schema before the flip for this service, allowing every capability an entry asks for"
#[test]
fn the_deployment_places_what_the_manifest_names() {
  let manifest = toml_of(&folder().join("tool.toml"));
  let crate_manifest = toml_of(&folder().join("Cargo.toml"));
  let binary = text(&crate_manifest["bin"][0], &["name"]);
  let workspace = toml_of(&folder().join("../../Cargo.toml"));
  let declaration = toml_of(&folder().join("../../testbed.toml"));
  let steps = declaration["deploy"]["step"].as_array().unwrap();
  let step = steps
    .iter()
    .find(|step| text(step, &["command"]).contains("tools/servers/.placed"))
    .expect("a step places the tool");
  let command = text(step, &["command"]);

  assert!(words(&workspace["workspace"]["members"]).contains(&"tools/servers"));
  assert_eq!(text(step, &["phase"]), "before-flip");
  assert!(words(&declaration["build"]["binaries"]).contains(&binary));
  assert!(command.contains(&format!("bin/{binary} ")), "{command}");
  assert!(command.contains("tools/servers/tool.toml "), "{command}");
  assert!(command.contains("--placed --service voice"), "{command}");

  for entry in manifest["entry"].as_array().unwrap() {
    let input = text(entry, &["input"]);

    assert!(command.contains(&format!("tools/servers/{input} ")), "{command}");

    for capability in words(&entry["capabilities"]) {
      assert!(command.contains(&format!("--allow {capability}")), "{command}");
    }
  }
}
