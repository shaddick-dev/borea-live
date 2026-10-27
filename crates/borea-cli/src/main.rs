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
fn help() {
    println!(
        "Borea backend\n  probe | catalog | disks | locales\n  net scan | net status\n  validate | review | render --plan PLAN.toml\n  dry-run --plan PLAN.toml --bootstrap BOOTSTRAP.json\n  install --plan PLAN.toml --approval DIGEST < credentials.json\n  events [CURSOR] | log | retry ACTION_ID\nCredentials must be supplied on stdin; they are never command options."
    );
}
