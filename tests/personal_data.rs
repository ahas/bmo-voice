//! No tracked file holds personal data: a mail address outside the reserved example domains and the
//! development address, a person's home path, a telephone number, a token or a private network address
//! (`PERSONAL-DATA.md`). The check carries the patterns only; no list of addresses, paths or values is kept in
//! the repository.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use regex::Regex;

/// The home folders a neutral example writes, so a person's own is not among them.
const NEUTRAL_HOME: [&str; 5] = ["user", "tester", "qa-admin", "placeholder", "example"];

/// The mail addresses the repository allows by name (`PERSONAL-DATA.md`, 「Allowed values」).
const ALLOWED_MAIL: [&str; 1] = ["dev@ahas.io"];

/// The frozen package list of the voice environment. The package manager wrote it, and a package version's
/// four parts can stand in the shape of a private address.
const PACKAGE_LIST: &str = "synth/voice-node-requirements.txt";

fn root() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every file git tracks or would track: the committed ones and the new ones it does not ignore.
fn tracked() -> Vec<String> {
  let output = Command::new("git")
    .args(["ls-files", "--cached", "--others", "--exclude-standard", "-z"])
    .current_dir(root())
    .stdin(Stdio::null())
    .output()
    .expect("git lists the files");

  assert!(
    output.status.success(),
    "git ls-files failed: {}",
    String::from_utf8_lossy(&output.stderr)
  );

  String::from_utf8(output.stdout)
    .expect("the paths are text")
    .split('\0')
    .filter(|path| !path.is_empty() && root().join(path).is_file())
    .map(str::to_owned)
    .collect()
}

/// The tracked files that read as text, with their text.
fn tracked_texts() -> Vec<(String, String)> {
  tracked()
    .into_iter()
    .filter_map(|path| std::fs::read_to_string(root().join(&path)).ok().map(|text| (path, text)))
    .collect()
}

/// Whether a home path's own folder is a neutral example rather than a person's.
fn is_neutral_home(folder: &str) -> bool {
  NEUTRAL_HOME.contains(&folder) || folder.starts_with("agent-")
}

/// Whether a mail address is the development address or stands in the reserved example space.
fn is_allowed_mail(address: &str) -> bool {
  ALLOWED_MAIL.contains(&address.to_lowercase().as_str())
    || address.rsplit_once('@').is_some_and(|(_, domain)| is_example_domain(domain))
}

/// Whether a domain is the reserved example space, where a neutral example address stands.
fn is_example_domain(domain: &str) -> bool {
  let domain = domain.to_lowercase();

  ["example.com", "example.net", "example.org"]
    .iter()
    .any(|example| domain == *example || domain.ends_with(&format!(".{example}")))
    || [".example", ".test", ".invalid", ".localhost"]
      .iter()
      .any(|suffix| domain.ends_with(suffix))
}

#[test]
fn no_personal_data_in_a_tracked_file() {
  let mail = Regex::new(concat!(r"\b[A-Za-z0-9._%+-]+@", r"[A-Za-z][A-Za-z0-9.-]*\.[A-Za-z]{2,}\b"))
    .expect("the mail pattern reads");
  let home = Regex::new(r"(?:/home/|/Users/|[A-Za-z]:\\Users\\)([A-Za-z0-9._-]+)")
    .expect("the home path pattern reads");
  let telephone = Regex::new(concat!(
    r"\b01[016789][-. ]?\d{3,4}[-. ]?\d{4}\b|",
    r"\+\d{1,3}[-. ]?\d{2,4}[-. ]?\d{3,4}[-. ]?\d{4}\b|",
    r"\+\d{9,15}\b"
  ))
  .expect("the telephone pattern reads");
  let token = Regex::new(concat!(
    r"gh[pousr]_[A-Za-z0-9]{20,}|",
    r"github_pat_[A-Za-z0-9_]{20,}|",
    r"sk-[A-Za-z0-9_-]{20,}|",
    r"xox[baprs]-[A-Za-z0-9-]{10,}|",
    r"AKIA[0-9A-Z]{16}|",
    r"AIza[0-9A-Za-z_-]{35}"
  ))
  .expect("the token pattern reads");
  let private = Regex::new(concat!(
    r"(?i)\b(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}|",
    r"192\.168\.\d{1,3}\.\d{1,3}|",
    r"172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3})\b|",
    r"\b(?:f[cd][0-9a-f]{2}|fe80):"
  ))
  .expect("the private address pattern reads");
  let mut found = Vec::new();

  for (path, text) in tracked_texts() {
    found.extend(
      mail
        .find_iter(&text)
        .filter(|address| !is_allowed_mail(address.as_str()))
        .map(|address| format!("{path}: mail address {}", address.as_str())),
    );
    found.extend(
      home
        .captures_iter(&text)
        .filter(|capture| !capture.get(1).is_some_and(|folder| is_neutral_home(folder.as_str())))
        .map(|capture| format!("{path}: home path {}", &capture[0])),
    );
    found.extend(
      telephone
        .find_iter(&text)
        .map(|number| format!("{path}: telephone number {}", number.as_str())),
    );
    found.extend(token.find_iter(&text).map(|token| format!("{path}: token {}", token.as_str())));
    found.extend(
      private
        .find_iter(&text)
        .filter(|_| path != PACKAGE_LIST)
        .map(|address| format!("{path}: private address {}", address.as_str())),
    );
  }

  assert!(found.is_empty(), "personal data in tracked files: {found:#?}");
}
