//! `borea`: the bootstrapper.
//!
//! Today it explains readiness: which installation environment this is, what the
//! machine looks like, how each Windpad target would be obtained from here, and
//! which desktops can be composed on editions shipped without one. It also brings
//! the machine online through integrated Wi-Fi. Verified download and launch of the
//! Windpad GUI come with milestone M1. Nothing here changes disks.
//!
//! On an installed system it adds and removes Borea's extras with the
//! distribution's own package manager (`borea install gaming`).

mod doctor;
mod extras;
mod shells;
mod ui;
mod wifi;

use std::io::{self, Write};
use std::process::ExitCode;

use ui::Out;

const USAGE: &str = "\
Borea — go wherever the wind takes you.

Usage: borea [COMMAND]

Commands:
  doctor                      Inspect this environment and machine, and every target's readiness (default)
  catalog                     List the Windpad families, distributions, and editions
  shells [DISTRO [COMPOSITOR]]
                              Compose a desktop: compositors and shells for editions without one
  wifi                        Connection status and networks in range
  wifi scan                   Rescan, then list networks
  wifi join SSID              Join a network; the passphrase is asked for, never passed as an argument
  wifi hidden SSID [wpa2|wpa3|open]
                              Join a hidden network
  extras                      The extras for this system (Gaming, Media codecs, Flatpak, Printing)
  install EXTRA… [--yes]      Add extras with the distribution's own package manager (as root)
  remove EXTRA… [--yes]       Remove what Borea added for those extras, nothing else (as root)
  help                        Show this message

`wifi join` adds a connection to this live session only.
Installing goes through the installer service: `borea-cli review --plan PLAN.json`, then
`borea-cli install --plan PLAN.json --approval DIGEST` with secrets as JSON on stdin.";

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&args, &mut io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        // Output piped into `head` and closed early: not an error worth reporting.
        Err(error) if broken_pipe(&*error) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("borea: {error}");
            ExitCode::FAILURE
        }
    }
}

fn broken_pipe(error: &(dyn std::error::Error + 'static)) -> bool {
    error.downcast_ref::<io::Error>().is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
}

fn run(args: &[&str], out: &mut Out) -> Result {
    match args {
        [] | ["doctor"] => doctor::run(out),
        ["catalog"] => doctor::catalog(out),
        ["shells", rest @ ..] if rest.len() <= 2 => shells::run(out, rest.first(), rest.get(1)),
        ["wifi"] => wifi::status(out, false),
        ["wifi", "scan"] => wifi::status(out, true),
        ["wifi", "join", ssid] => wifi::join(out, ssid),
        ["wifi", "hidden", ssid, security @ ..] if security.len() <= 1 => {
            wifi::hidden(out, ssid, security.first().copied().unwrap_or("wpa2"))
        }
        ["extras"] => extras::list(out),
        ["install", rest @ ..] => extras::install(out, rest),
        ["remove", rest @ ..] => extras::remove(out, rest),
        ["help" | "-h" | "--help"] => Ok(writeln!(out, "{USAGE}")?),
        ["-V" | "--version"] => Ok(writeln!(out, "borea {}", env!("CARGO_PKG_VERSION"))?),
        _ => Err(format!("unrecognized command `{}`\n\n{USAGE}", args.join(" ")).into()),
    }
}
