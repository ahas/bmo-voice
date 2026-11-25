//! The script studio's server (`docs/script-studio.md`, 「Access and service」): the voice's settings for the account
//! service and the studio's deployment values, its port among them, and its environment for the data root, the port
//! and the generated screen: `BMO_VOICE_STUDIO_DATA`, `BMO_VOICE_STUDIO_PORT` and `BMO_VOICE_STUDIO_STATIC`. Started
//! as `script-studio address`, it prints the address it would listen at and starts nothing.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use bmo_voice::settings::Settings;
use bmo_voice::studio::handler::{PREFIX, Script, router};
use bmo_voice::studio::records::{Records, Root};
use bmo_voice::studio::subtitles::Speech;
use bmo_voice::units::Caller;
use bmo_voice::units::web::Web;
use tokio::net::TcpListener;

fn named(variable: &str) -> Option<String> {
  std::env::var(variable).ok().filter(|value| !value.is_empty())
}

fn fail(why: impl std::fmt::Display) -> ExitCode {
  eprintln!("script-studio: {why}");
  ExitCode::FAILURE
}

#[tokio::main]
async fn main() -> ExitCode {
  let settings = match Settings::load() {
    Ok(settings) => settings,
    Err(why) => return fail(why),
  };
  let port = settings
    .studio
    .listen_port(named("BMO_VOICE_STUDIO_PORT").and_then(|port| port.parse().ok()));

  // `address` is the deployment's health's one question: where the server listens.
  if std::env::args().nth(1).as_deref() == Some("address") {
    println!("http://127.0.0.1:{port}");
    return ExitCode::SUCCESS;
  }

  let root =
    named("BMO_VOICE_STUDIO_DATA").map_or_else(|| settings.voice_folder().join("studio"), PathBuf::from);
  let static_dir = match named("BMO_VOICE_STUDIO_STATIC") {
    Some(named) => PathBuf::from(named),
    None => match Settings::release_folder() {
      Ok(release) => release.join("studio/client/.output/public"),
      Err(why) => return fail(format!("{why}; name the screen in BMO_VOICE_STUDIO_STATIC")),
    },
  };

  if let Err(fault) = std::fs::create_dir_all(&root) {
    return fail(format!("{} could not be made: {fault}", root.display()));
  }

  let records = match Records::open(Root::new(&root, settings.studio.source_root.as_deref())).await {
    Ok(records) => records,
    Err(fault) => return fail(format!("the review database could not be opened: {fault}")),
  };
  let web = match Caller::new("web", &settings, &settings.units.web, reqwest::Client::new()) {
    Ok(caller) => Web::new(caller),
    Err(why) => return fail(why),
  };
  let speech = Speech::new(&settings.studio.speakers);
  let script = Arc::new(Script::new(
    records,
    web,
    static_dir,
    speech,
    settings.studio.source_url.clone(),
    bmo_voice::now_ms,
  ));
  let listener = match TcpListener::bind(("127.0.0.1", port)).await {
    Ok(listener) => listener,
    Err(fault) => return fail(format!("127.0.0.1:{port} could not be listened on: {fault}")),
  };
  // A port of 0 is one the system chooses, named here as the server starts answering.
  let port = listener.local_addr().map_or(port, |at| at.port());

  println!("script studio: http://127.0.0.1:{port}{PREFIX}/");

  match axum::serve(listener, router(script).into_make_service_with_connect_info::<SocketAddr>()).await {
    Ok(()) => ExitCode::SUCCESS,
    Err(fault) => fail(fault),
  }
}
