#![forbid(unsafe_code)]
use borea_core::{Hint, Plan, hint};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
};
use zeroize::Zeroizing;
fn error(s: impl Into<String>) -> Hint {
    hint("cli", s)
}
fn rpc(method: &str, params: Value) -> Result<Value, Hint> {
    borea_protocol::call(method, &params, std::time::Duration::from_secs(600))
        .map_err(|e| error(e.to_string()))
}
fn rpc_install(plan: &Plan, approval: &str, raw: &str) -> Result<Value, Hint> {
    let mut socket = UnixStream::connect(borea_protocol::SOCKET)
        .map_err(|_| error("The installer service is not running."))?;
    let plan = serde_json::to_string(plan).map_err(|e| error(e.to_string()))?;
    let approval = serde_json::to_string(approval).map_err(|e| error(e.to_string()))?;
    socket.write_all(format!("{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"{}\",\"params\":{{\"plan\":{plan},\"approval\":{approval},\"secrets\":", borea_protocol::method::INSTALL_START).as_bytes()).map_err(|e|error(e.to_string()))?;
    socket.write_all(raw.as_bytes()).map_err(|e| error(e.to_string()))?;
    socket.write_all(b"}}\n").map_err(|e| error(e.to_string()))?;
    borea_protocol::read_response(socket).map_err(|e| error(e.to_string()))
}
fn read_plan(path: &str) -> Result<Plan, Hint> {
    let text = fs::read_to_string(path).map_err(|_| error("The plan file cannot be read."))?;
    let p: Plan = if path.ends_with(".json") {
        serde_json::from_str(&text).map_err(|_| error("The JSON plan is invalid."))?
    } else {
        toml::from_str(&text).map_err(|_| error("The TOML plan is invalid."))?
    };
    p.validate_intent()?;
    borea_locale::validate(&p.locale)?;
    Ok(p)
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(v) => {
            if let Some(v) = v {
                match serde_json::to_string_pretty(&v) {
                    Ok(text) => println!("{text}"),
                    Err(_) => return std::process::ExitCode::FAILURE,
                }
            }
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("borea-cli: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}
fn run() -> Result<Option<Value>, Hint> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = args.first().map(String::as_str) else {
        help();
        return Ok(None);
    };
    match command {
        "help" | "--help" => {
            help();
            Ok(None)
        }
        "probe" => {
            let hw = borea_catalog::probe::probe().map_err(|e| error(e.to_string()))?;
            let v = serde_json::from_str(&borea_catalog::probe::report(&hw))
                .map_err(|_| error("Invalid hardware report."))?;
            Ok(Some(v))
        }
        "catalog" => Ok(Some(
            serde_json::from_str(&borea_catalog::export::json())
                .map_err(|_| error("Invalid catalog."))?,
        )),
        "disks" => {
            Ok(Some(serde_json::to_value(borea_disk::probe()?).map_err(|e| error(e.to_string()))?))
        }
        "locales" => Ok(Some(
            json!({"languages":borea_locale::LOCALES,"keyboards":borea_locale::KEYBOARDS,"timezones":borea_locale::TIMEZONES}),
        )),
        "net" => {
            let method = match args.get(1).map(String::as_str) {
                Some("scan") => borea_protocol::method::NET_SCAN,
                Some("status") => borea_protocol::method::NET_STATUS,
                _ => {
                    return Err(error(
                        "Use net scan or net status; credentials are accepted by the service on stdin only.",
                    ));
                }
            };
            Ok(Some(rpc(method, Value::Null)?))
        }
        "validate" | "review" | "render" | "install" | "dry-run" => {
            let mut options = std::collections::BTreeMap::new();
            let mut iter = args.iter().skip(1);
            while let Some(key) = iter.next() {
                if !["--plan", "--bootstrap", "--approval"].contains(&key.as_str())
                    || options.contains_key(key.as_str())
                {
                    return Err(error("Unknown or duplicate command option."));
                }
                let value = iter
                    .next()
                    .filter(|s| !s.starts_with("--"))
                    .ok_or_else(|| error("An option value is missing."))?;
                options.insert(key.as_str(), value);
            }
            let path = options
                .get("--plan")
                .ok_or_else(|| error("Select an explicit plan with --plan."))?;
            let plan = read_plan(path)?;
            if command == "dry-run" {
                let path = options.get("--bootstrap").ok_or_else(|| {
                    error(
                        "A dry run needs --bootstrap with the reviewed native tool-root manifest.",
                    )
                })?;
                let bytes =
                    fs::read(path).map_err(|_| error("The bootstrap manifest cannot be read."))?;
                let bootstrap: borea_backend::Bootstrap = serde_json::from_slice(&bytes)
                    .map_err(|_| error("The bootstrap manifest is invalid."))?;
                let hw = borea_catalog::probe::probe().map_err(|e| error(e.to_string()))?;
                let job = borea_job::compile(&plan, &hw, &borea_disk::probe()?, &bootstrap)?;
                return Ok(Some(
                    json!({"approval":job.digest()?,"script":job.render_script(),"review":job.review}),
                ));
            }
            if command == "install" {
                let approval = options
                    .get("--approval")
                    .ok_or_else(|| error("Supply the approval digest returned by Review."))?;
                let mut raw = Zeroizing::new(String::new());
                std::io::stdin()
                    .take(65537)
                    .read_to_string(&mut raw)
                    .map_err(|_| error("Credentials could not be read from stdin."))?;
                if raw.len() > 65536 {
                    return Err(error("The credentials input is too large."));
                }
                // Do not clone or log the credential object. Its wire buffer is wiped on drop.
                let credentials: borea_core::Secrets = serde_json::from_str(&raw)
                    .map_err(|_| error("Supply the credentials JSON on stdin."))?;
                credentials.validate(&plan.user)?;
                Ok(Some(rpc_install(&plan, approval, &raw)?))
            } else {
                let method = match command {
                    "validate" => borea_protocol::method::PLAN_VALIDATE,
                    "review" => borea_protocol::method::PLAN_REVIEW,
                    _ => borea_protocol::method::PLAN_RENDER,
                };
                Ok(Some(rpc(method, json!({"plan":plan}))?))
            }
        }
        "handoff" => handoff(&args[1..]),
        "events" => Ok(Some(rpc(
            borea_protocol::method::INSTALL_EVENTS,
            json!({"after":args.get(1).and_then(|s|s.parse::<usize>().ok()).unwrap_or(0)}),
        )?)),
        "log" => Ok(Some(rpc(borea_protocol::method::INSTALL_LOG, Value::Null)?)),
        "retry" => Ok(Some(rpc(
            borea_protocol::method::INSTALL_RETRY,
            json!({"action_id":args.get(1).ok_or_else(||error("Supply the failed action ID."))?}),
        )?)),
        _ => Err(error("Unknown command. Run borea-cli help.")),
    }
}
enum Shown {
    Going,
    Done,
    Failed(String),
}
/// Print one install/hand-off event (borea-exec's `{"event": …}` wire shape).
fn show(event: &Value) -> Shown {
    match event["event"].as_str() {
        Some("stage-started") => eprintln!("==> {}", event["title"].as_str().unwrap_or_default()),
        Some("line") => eprintln!("    {}", event["text"].as_str().unwrap_or_default()),
        Some("failed") => {
            return Shown::Failed(
                event["fault"]["title"].as_str().unwrap_or("The hand-off failed.").to_owned(),
            );
        }
        Some("finished") => {
            eprintln!("==> Restarting into the installer.");
            return Shown::Done;
        }
        _ => {}
    }
    Shown::Going
}
/// `60G`, `60GiB` or `64424509440` (bytes).
fn size(text: &str) -> Result<u64, Hint> {
    let lower = text.to_ascii_lowercase();
    let (number, unit) = match lower.trim_end_matches("ib").trim_end_matches('b') {
        n if n.ends_with('g') => (&n[..n.len() - 1], borea_core::GIB),
        n if n.ends_with('t') => (&n[..n.len() - 1], 1024 * borea_core::GIB),
        n => (n, 1),
    };
    number
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(unit))
        .filter(|n| *n > 0)
        .ok_or_else(|| error("Give the size as 60G, 60GiB or bytes."))
}
/// `handoff TARGET (--whole DISK | --alongside WINDOWS_PARTITION --size SIZE) [--extras ID,ID] [--approval DIGEST]`
fn handoff(args: &[String]) -> Result<Option<Value>, Hint> {
    let target = args.first().filter(|t| !t.starts_with("--")).ok_or_else(|| {
        error("Name the installer to hand off to, e.g. borea-cli handoff omarchy --whole /dev/nvme0n1")
    })?;
    let mut options = std::collections::BTreeMap::new();
    let mut iter = args.iter().skip(1);
    while let Some(key) = iter.next() {
        if !["--whole", "--alongside", "--size", "--extras", "--approval"].contains(&key.as_str())
            || options.contains_key(key.as_str())
        {
            return Err(error("Unknown or duplicate handoff option."));
        }
        let value = iter
            .next()
            .filter(|s| !s.starts_with("--"))
            .ok_or_else(|| error("An option value is missing."))?;
        options.insert(key.as_str(), value.as_str());
    }
    let facts = borea_disk::probe()?;
    let disk = match (options.get("--whole"), options.get("--alongside")) {
        (Some(path), None) => {
            let d =
                facts.disks.iter().find(|d| d.id.path.as_os_str() == *path).ok_or_else(|| {
                    error(format!("{path} is not a disk here; run borea-cli disks."))
                })?;
            borea_core::DiskPlan::Whole { disk: d.id.clone() }
        }
        (None, Some(path)) => {
            let linux_size = size(options.get("--size").ok_or_else(|| {
                error("Say how much space to give it beside Windows, e.g. --size 60G.")
            })?)?;
            let (d, p) = facts
                .disks
                .iter()
                .find_map(|d| d.parts.iter().find(|p| p.path.as_os_str() == *path).map(|p| (d, p)))
                .ok_or_else(|| {
                    error(format!("{path} is not a partition here; run borea-cli disks."))
                })?;
            borea_core::DiskPlan::Alongside {
                disk: d.id.clone(),
                windows_part: borea_core::PartId {
                    path: p.path.clone(),
                    uuid: p.uuid.clone(),
                    number: p.number,
                },
                linux_size,
            }
        }
        _ => {
            return Err(error(
                "Choose exactly one: --whole DISK, or --alongside WINDOWS_PARTITION --size SIZE.",
            ));
        }
    };
    let extras: Vec<&str> = options
        .get("--extras")
        .map(|list| list.split(',').filter(|e| !e.is_empty()).collect())
        .unwrap_or_default();
    let plan = json!({"version":1,"target":target,"disk":disk,"extras":extras});
    let Some(approval) = options.get("--approval") else {
        let review = rpc(borea_protocol::method::HANDOFF_REVIEW, json!({"handoff":plan}))?;
        eprintln!(
            "Nothing has changed yet. To go ahead, run the same command with --approval {}",
            review["approval"].as_str().unwrap_or_default()
        );
        return Ok(Some(review));
    };
    rpc(borea_protocol::method::HANDOFF_START, json!({"handoff":plan,"approval":approval}))?;
    let mut cursor = 0;
    loop {
        let batch = rpc(borea_protocol::method::INSTALL_EVENTS, json!({"after":cursor}))?;
        for event in batch["events"].as_array().into_iter().flatten() {
            match show(event) {
                Shown::Done => return Ok(None),
                Shown::Failed(why) => return Err(error(why)),
                Shown::Going => {}
            }
        }
        cursor = batch["cursor"].as_u64().unwrap_or(cursor as u64) as usize;
        if batch["more"].as_bool() != Some(true) {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}
fn help() {
    println!(
        "Borea backend\n  probe | catalog | disks | locales\n  net scan | net status\n  validate | review | render --plan PLAN.toml\n  dry-run --plan PLAN.toml --bootstrap BOOTSTRAP.json\n  install --plan PLAN.toml --approval DIGEST < credentials.json\n  events [CURSOR] | log | retry ACTION_ID\n  handoff omarchy --whole DISK [--extras gaming,codecs,flatpak,printing] [--approval DIGEST]\n  handoff omarchy --alongside WINDOWS_PARTITION --size 60G [--extras …] [--approval DIGEST]\nCredentials must be supplied on stdin; they are never command options."
    );
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn reads_sizes_and_events() {
        assert_eq!(size("60G").unwrap(), 60 * borea_core::GIB);
        assert_eq!(size("60GiB").unwrap(), 60 * borea_core::GIB);
        assert_eq!(size("1T").unwrap(), 1024 * borea_core::GIB);
        assert_eq!(size("1000").unwrap(), 1000);
        assert!(size("lots").is_err() && size("0G").is_err());
        let wire = |e: borea_exec::Event| serde_json::to_value(e).unwrap();
        assert!(matches!(show(&wire(borea_exec::Event::Finished)), Shown::Done));
        assert!(matches!(
            show(&wire(borea_exec::Event::StageStarted { stage: 0, title: "x".into() })),
            Shown::Going
        ));
        let failed = borea_exec::Event::Failed {
            fault: borea_exec::Fault {
                action_id: "handoff".into(),
                title: "No space".into(),
                command: String::new(),
                exit_code: None,
                stderr_tail: vec![],
                suggestion: String::new(),
                retryable: false,
            },
        };
        assert!(matches!(show(&wire(failed)), Shown::Failed(why) if why == "No space"));
    }
}
