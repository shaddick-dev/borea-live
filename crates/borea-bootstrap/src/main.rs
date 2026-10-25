//! `borea`: the bootstrapper.
//!
//! Today it explains readiness: which installation environment this is, what the
//! machine looks like, how each Windpad target would be obtained from here, and
//! which desktops can be composed on editions shipped without one. It also brings
//! the machine online through integrated Wi-Fi. Verified download and launch of the
//! Windpad GUI come with milestone M1. Nothing here changes disks.

mod doctor;
mod plan;
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
  targets                     Show the four rolling online target contracts and execution availability
  shells [DISTRO [COMPOSITOR]]
                              Compose a desktop: compositors and shells for editions without one
  wifi                        Connection status and networks in range
  wifi scan                   Rescan, then list networks
  wifi join SSID              Join a network; the passphrase is asked for, never passed as an argument
  wifi hidden SSID [wpa2|wpa3|open]
                              Join a hidden network
  plan [--distro arch] --disk NAME --user NAME [options]
                              Plan an Arch Linux install: resolve, verify, and review it; nothing is written
  help                        Show this message

Planning changes nothing. `wifi join` adds a connection to this live session only.
Installing is a separate, approved step: `sudo borea-worker PLAN --approve DIGEST`.";

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
        ["targets"] => Ok(writeln!(out, "{}", borea_engine::online::profiles_json())?),
        ["shells", rest @ ..] if rest.len() <= 2 => shells::run(out, rest.first(), rest.get(1)),
        ["wifi"] => wifi::status(out, false),
        ["wifi", "scan"] => wifi::status(out, true),
        ["wifi", "join", ssid] => wifi::join(out, ssid),
        ["wifi", "hidden", ssid, security @ ..] if security.len() <= 1 => {
            wifi::hidden(out, ssid, security.first().copied().unwrap_or("wpa2"))
        }
        ["plan", rest @ ..] => plan::run(out, rest),
        ["help" | "-h" | "--help"] => Ok(writeln!(out, "{USAGE}")?),
        ["-V" | "--version"] => Ok(writeln!(out, "borea {}", env!("CARGO_PKG_VERSION"))?),
        _ => Err(format!("unrecognized command `{}`\n\n{USAGE}", args.join(" ")).into()),
    }
}
