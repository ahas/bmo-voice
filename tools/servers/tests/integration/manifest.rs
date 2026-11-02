//! The tool's manifest and input schema held to the executable (bmo-pocket's `docs/api.md`, 「The manifest」;
//! docs/boundary-api.md, 「The hosted tool」): the tool named for its folder, its executable the crate's own
//! binary, its one entry the one the executable answers, its schema an empty object, and the deployment placing
//! what the manifest names.

use std::path::{Path, PathBuf};

use serde_json::Value;
use voice_servers_tool::{ENTRY, PROTOCOL};

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

// "its one entry is the one the executable answers, read-only, asking for the capability room.tools, with no span
// declared for its call"
#[test]
fn its_one_entry_is_the_one_the_executable_answers() {
  let manifest = toml_of(&folder().join("tool.toml"));
  let entries = manifest["entry"].as_array().unwrap();
  let entry = &entries[0];

  assert_eq!(entries.len(), 1);
  assert_eq!(format!("{}/{}", text(entry, &["namespace"]), text(entry, &["name"])), ENTRY);
  assert_eq!(words(&entry["capabilities"]), ["room.tools"]);
  assert!(entry.get("grant").is_none());
  assert!(entry.get("system").is_none());
  assert!(entry.get("timeout").is_none());
  assert!(entry.get("unbounded").is_none());
  assert_eq!(entry["hints"]["read_only"].as_bool(), Some(true));
  assert!(!text(entry, &["description"]).is_empty());
  assert!(manifest.get("resources").is_none());
}

// "the schema is an empty object: the entry takes no arguments"
#[test]
fn the_schema_is_an_empty_object() {
  let manifest = toml_of(&folder().join("tool.toml"));
  let input = text(&manifest["entry"][0], &["input"]);
  let schema: Value =
    serde_json::from_str(&std::fs::read_to_string(folder().join(input)).unwrap()).unwrap();

  assert_eq!(input, "servers.json");
  assert_eq!(schema["type"], "object");
  assert_eq!(schema["additionalProperties"], false);
  assert!(schema["properties"].as_object().unwrap().is_empty());
  assert!(schema.get("required").is_none());
}

// "the workspace lists the crate, the release builds its binary, and the deployment places the binary, the
// manifest and the schema before the flip for this service, allowing the capability the entry asks for"
#[test]
fn the_deployment_places_what_the_manifest_names() {
  let manifest = toml_of(&folder().join("tool.toml"));
  let crate_manifest = toml_of(&folder().join("Cargo.toml"));
  let binary = text(&crate_manifest["bin"][0], &["name"]);
  let input = text(&manifest["entry"][0], &["input"]);
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
  assert!(command.contains(&format!("tools/servers/{input} ")), "{command}");
  assert!(command.contains("--placed --service voice"), "{command}");

  for capability in words(&manifest["entry"][0]["capabilities"]) {
    assert!(command.contains(&format!("--allow {capability}")), "{command}");
  }
}
