//! A long line through the voice server's own `POST /speak` comes back as one sound (`tests/mouth_long_line.py`),
//! run on the voice environment's interpreter: the one `BMO_VOICE_PYTHON` names, else `bmo-voice/.venv` under the
//! account's home (docs/voice-plan.md, 「The environments」). The home is the account's own, read from the password
//! database, since a suite may run with `HOME` set to a scratch folder of its own.

use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::Command;

// The home the password database names for the account this test runs as.
fn account_home() -> PathBuf {
  // SAFETY: getpwuid_r writes only into the record and the buffer handed to it, and the home is copied out of the
  // buffer before the buffer goes.
  unsafe {
    let mut record: libc::passwd = std::mem::zeroed();
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    let said = libc::getpwuid_r(libc::getuid(), &mut record, buffer.as_mut_ptr(), buffer.len(), &mut found);

    assert!(said == 0 && !found.is_null(), "the password database names no home for this account");

    PathBuf::from(OsStr::from_bytes(CStr::from_ptr(record.pw_dir).to_bytes()))
  }
}

fn voice_python() -> PathBuf {
  match std::env::var_os("BMO_VOICE_PYTHON") {
    Some(named) => PathBuf::from(named),
    None => account_home().join("bmo-voice/.venv/bin/python"),
  }
}

#[test]
fn a_line_of_any_length_the_engine_takes_in_comes_back_as_one_sound() {
  let python = voice_python();
  let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mouth_long_line.py");
  let ran = Command::new(&python).arg(&script).output().unwrap_or_else(|why| {
    panic!("the voice environment's interpreter {} does not run: {why}", python.display())
  });

  assert!(
    ran.status.success(),
    "the long line was not spoken whole:\n{}{}",
    String::from_utf8_lossy(&ran.stdout),
    String::from_utf8_lossy(&ran.stderr)
  );
}
