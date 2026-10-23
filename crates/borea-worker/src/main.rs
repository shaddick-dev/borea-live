//! `borea-worker`: carry out one approved installation plan, as root.
//!
//! ```text
//! borea-worker PLAN.json --approve DIGEST [--bootstrap ARCHIVE] [--verbose]
//! ```
//!
//! It refuses unless DIGEST is the plan's own, re-checks the disk the plan names
//! just before erasing it, runs in a private mount namespace (every mount it
//! makes disappears with it), and asks for the new user's password itself.
//! Everything it runs, and their output, goes to /run/borea/install.log; the
//! password never does.

mod arch;
mod run;

use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use borea_engine::InstallPlan;

const USAGE: &str =
    "usage: borea-worker PLAN.json --approve DIGEST [--bootstrap ARCHIVE] [--verbose]";
/// Set in the re-executed worker, inside its own mount namespace.
const IN_NAMESPACE: &str = "BOREA_WORKER_NAMESPACE";

fn main() -> ExitCode {
    match start() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("borea-worker: {error}");
            ExitCode::FAILURE
        }
    }
}

fn start() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    let value =
        |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let plan_path = args.first().filter(|a| !a.starts_with("--")).ok_or(USAGE)?;
    let approved = value("--approve").ok_or(USAGE)?;
    let bootstrap = value("--bootstrap").map(PathBuf::from);
    let verbose = args.iter().any(|a| a == "--verbose");

    if !rustix::process::geteuid().is_root() {
        return Err("installing needs root: run it with sudo".into());
    }
    let text = std::fs::read_to_string(plan_path).map_err(|e| format!("{plan_path}: {e}"))?;
    let plan = InstallPlan::from_json(&text)?;
    if plan.digest() != approved {
        return Err(
            "this plan is not the one approved: its digest differs. Plan and review again.".into(),
        );
    }

    // Everything mounted from here on lives in a namespace of its own.
    if std::env::var_os(IN_NAMESPACE).is_none() {
        let status = Command::new("/usr/bin/unshare")
            .args(["--mount", "--propagation", "private", "--"])
            .arg(std::env::current_exe().map_err(|e| e.to_string())?)
            .args(&args)
            .env(IN_NAMESPACE, "1")
            .status()
            .map_err(|e| format!("unshare: {e}"))?;
        return if status.success() {
            Ok(())
        } else {
            Err("the installation did not finish".into())
        };
    }

    let password = password(&plan.system.username)?;
    let log = run::Log::open(verbose)?;
    match plan.distro.as_str() {
        "arch" => arch::Install::new(&plan, &password, bootstrap, log).run(),
        other => Err(format!("no installer for `{other}` yet")),
    }
}

/// The new user's password: asked twice on a terminal, else one line of stdin.
fn password(user: &str) -> Result<String, String> {
    if std::io::stdin().is_terminal() {
        let first = rpassword::prompt_password(format!("Password for {user}: "))
            .map_err(|e| e.to_string())?;
        let again = rpassword::prompt_password("Again: ").map_err(|e| e.to_string())?;
        if first != again {
            return Err("the passwords differ".into());
        }
        return check_password(first);
    }
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).map_err(|e| e.to_string())?;
    check_password(line.trim_end_matches(['\r', '\n']).to_owned())
}

fn check_password(password: String) -> Result<String, String> {
    if password.is_empty() {
        return Err("an empty password is not allowed".into());
    }
    if password.contains('\n') {
        return Err("a password can't contain a newline".into());
    }
    Ok(password)
}
