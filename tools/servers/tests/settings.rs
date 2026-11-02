//! Where the tool reads the voice's settings (docs/boundary-api.md, 「The hosted tool」): the file
//! `BMO_VOICE_SETTINGS` names, else the one under the deploy tree's link below the home.

use std::ffi::OsString;
use std::path::PathBuf;

use voice_servers_tool::{DEPLOY_SETTINGS, settings_path};

fn os(text: &str) -> Option<OsString> {
  Some(OsString::from(text))
}

// "the file the variable names comes first"
#[test]
fn the_file_the_variable_names_comes_first() {
  assert_eq!(
    settings_path(os("/srv/voice/settings.toml"), os("/srv/account")).unwrap(),
    PathBuf::from("/srv/voice/settings.toml")
  );
  assert_eq!(settings_path(os("named.toml"), None).unwrap(), PathBuf::from("named.toml"));
}

// "else the deploy tree's link's settings below the home, a variable set to nothing naming none"
#[test]
fn else_the_deploy_trees_settings_below_the_home() {
  let deployed = PathBuf::from("/srv/account/.local/share/bmo-voice/deploy/settings.toml");

  assert_eq!(DEPLOY_SETTINGS, ".local/share/bmo-voice/deploy/settings.toml");
  assert_eq!(settings_path(None, os("/srv/account")).unwrap(), deployed);
  assert_eq!(settings_path(os(""), os("/srv/account")).unwrap(), deployed);
}

// "with neither, the settings stand nowhere, and the refusal names both"
#[test]
fn with_neither_the_settings_stand_nowhere() {
  for (named, home) in [(None, None), (os(""), os(""))] {
    let refusal = settings_path(named, home).unwrap_err();

    assert!(refusal.contains("BMO_VOICE_SETTINGS"), "{refusal}");
    assert!(refusal.contains("HOME"), "{refusal}");
  }
}
