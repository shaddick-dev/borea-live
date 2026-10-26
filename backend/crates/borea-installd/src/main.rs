#![forbid(unsafe_code)]
mod protocol;
use borea_core::{Hint, Plan, Secrets, hint};
use borea_exec::{Event, Session};
use borea_net::NetworkBackend;
use nix::{
    sys::socket::{getsockopt, sockopt::PeerCredentials},
    unistd::{Uid, User},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
use zeroize::Zeroizing;
const SOCKET: &str = "/run/borea/installd.sock";
const MAX_REQUEST: u64 = 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request<'a> {
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default, borrow)]
    params: Option<&'a serde_json::value::RawValue>,
}
#[derive(Default)]
struct State {
    events: VecDeque<Event>,
    event_base: usize,
    pending_ssid: Option<String>,
    fault: Option<borea_exec::Fault>,
    worker: Option<mpsc::Sender<String>>,
    busy: bool,
    job_id: Option<String>,
}
impl State {
    fn push(&mut self, event: Event) {
        if let Event::Failed { fault } = &event {
            self.fault = Some(fault.clone());
        }
        if self.events.len() == 4096 {
            self.events.pop_front();
            self.event_base += 1;
        }
        self.events.push_back(event);
    }
    fn events(&self, after: usize) -> Result<Value, Hint> {
        let cursor = self.event_base + self.events.len();
        if after > cursor {
            return Err(fail("The event cursor is invalid."));
        }
        let events =
            self.events.iter().skip(after.saturating_sub(self.event_base)).collect::<Vec<_>>();
        Ok(
            json!({"events":events,"cursor":cursor,"truncated":after<self.event_base,"busy":self.busy,"job_id":self.job_id}),
        )
    }
}
struct Backend {
    state: Arc<Mutex<State>>,
    allowed_uid: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanParams {
    plan: Plan,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartParams {
    plan: Plan,
    secrets: Secrets,
    approval: String,
}
fn fail(message: impl Into<String>) -> Hint {
    hint("service", message)
}
fn to_value(v: impl serde::Serialize) -> Result<Value, Hint> {
    serde_json::to_value(v).map_err(|e| fail(e.to_string()))
}
fn params<T: for<'de> Deserialize<'de>>(value: &str) -> Result<T, Hint> {
    serde_json::from_str(value).map_err(|_| fail("The request parameters are invalid."))
}
fn bootstrap(plan: &Plan) -> Result<borea_backend::Bootstrap, Hint> {
    if !Path::new("/etc/borea/bootstrap.json").exists() {
        return borea_backend::bootstrap::resolve(plan);
    }
    let data = fs::read_to_string("/etc/borea/bootstrap.json")
        .map_err(|_| fail("The signed upstream tool-root catalog is not configured."))?;
    let entries: Vec<borea_backend::Bootstrap> = serde_json::from_str(&data)
        .map_err(|_| fail("The upstream tool-root catalog is invalid."))?;
    entries.into_iter().find(|b| b.distro == plan.distro && b.release == plan.release).ok_or_else(
        || fail("No verified tool root is available for this distribution and release."),
    )
}
fn prepare(plan: &Plan) -> Result<borea_job::Job, Hint> {
    borea_locale::validate(&plan.locale)?;
    let hw = borea_catalog::probe::probe().map_err(|e| fail(e.to_string()))?;
    let facts = borea_disk::probe()?;
    let mut job = borea_job::compile(plan, &hw, &facts, &bootstrap(plan)?)?;
    borea_job::resolve_external(&mut job)?;
    Ok(job)
}
fn field<T: for<'de> Deserialize<'de>>(plan: &Value, name: &str) -> Result<T, Hint> {
    serde_json::from_value(
        plan.get(name).cloned().ok_or_else(|| hint(name, "Make this choice before continuing."))?,
    )
    .map_err(|_| hint(name, "This choice is invalid."))
}
fn validate_step(plan: &Value, step: borea_core::Step) -> Result<(), Hint> {
    use borea_core::Step;
    match step {
        Step::Network => {
            let choice: borea_core::NetChoice = field(plan, "net")?;
            if let borea_core::NetChoice::Wifi { ssid } | borea_core::NetChoice::Hidden { ssid } =
                choice
            {
                if ssid.is_empty() || ssid.len() > 32 || ssid.contains('\0') {
                    return Err(hint("ssid", "Choose a valid network name."));
                }
            }
            let wifi = borea_net::nm::Wifi::system().map_err(|e| fail(e.to_string()))?;
            if wifi.check_connectivity().map_err(|e| fail(e.to_string()))?
                != borea_net::nm::Connectivity::Full
            {
                return Err(hint("network", "Connect to the internet before continuing."));
            }
            Ok(())
        }
        Step::Locale => borea_locale::validate(&field(plan, "locale")?),
        Step::Distro => {
            let distro: borea_core::Distribution = field(plan, "distro")?;
            let release: String = field(plan, "release")?;
            let valid = match distro {
                borea_core::Distribution::Arch => release == "rolling",
                borea_core::Distribution::Debian => release == "trixie",
                borea_core::Distribution::Fedora => {
                    release.parse::<u32>().is_ok_and(|n| (40..=99).contains(&n))
                }
                borea_core::Distribution::OpenSuse => release == "tumbleweed",
            };
            if valid {
                Ok(())
            } else {
                Err(hint("release", "Choose a supported upstream release."))
            }
        }
        Step::Desktop => {
            let _: Option<borea_core::Desktop> = field(plan, "desktop")?;
            Ok(())
        }
        Step::System => {
            let system: borea_core::SystemChoice = serde_json::from_value(plan.clone())
                .map_err(|_| hint("system", "Complete the system profile choices."))?;
            system.validate()
        }
        Step::Disk => {
            let disk: borea_core::DiskPlan = field(plan, "disk")?;
            let distro = field(plan, "distro")?;
            let desktop: Option<borea_core::Desktop> = field(plan, "desktop")?;
            borea_disk::layout(&disk, &borea_disk::probe()?, distro, desktop.is_some()).map(|_| ())
        }
        Step::You => field::<borea_core::Identity>(plan, "user")?.validate(),
        _ => {
            let plan: Plan = serde_json::from_value(plan.clone())
                .map_err(|_| fail("Complete every choice before Review."))?;
            plan.validate_intent()?;
            borea_locale::validate(&plan.locale)?;
            borea_core::review(
                &plan,
                &borea_catalog::probe::probe().map_err(|e| fail(e.to_string()))?,
            )?;
            borea_disk::layout(
                &plan.disk,
                &borea_disk::probe()?,
                plan.distro,
                plan.desktop.is_some(),
            )
            .map(|_| ())
        }
    }
}
impl Backend {
    fn call(&self, method: &str, input: &str) -> Result<Value, Hint> {
        match method {
            "system.methods" => to_value(protocol::METHODS),
            "system.info" => {
                let hw = borea_catalog::probe::probe().map_err(|e| fail(e.to_string()))?;
                let cmdline = fs::read_to_string("/proc/cmdline").unwrap_or_default();
                Ok(
                    json!({"version":env!("CARGO_PKG_VERSION"),"uefi":Path::new("/sys/firmware/efi").is_dir(),"secure_boot":hw.secure_boot,"motion":!cmdline.split_whitespace().any(|s|s=="borea.motion=0")}),
                )
            }
            "hardware.get" => {
                let hw = borea_catalog::probe::probe().map_err(|e| fail(e.to_string()))?;
                serde_json::from_str(&borea_catalog::probe::report(&hw))
                    .map_err(|_| fail("The hardware report is invalid."))
            }
            "catalog.get" => serde_json::from_str(&borea_catalog::export::json())
                .map_err(|_| fail("The application catalog is invalid.")),
            "locale.languages" => to_value(borea_locale::LOCALES),
            "locale.keyboards" => to_value(borea_locale::KEYBOARDS),
            "locale.timezones" => to_value(borea_locale::TIMEZONES),
            "geo.suggest" => Ok(match borea_net::geo_suggest() {
                Some(geo) => {
                    json!({"country":geo.country_code,"timezone":geo.time_zone,"defaults":borea_locale::suggest(&geo.country_code)})
                }
                None => Value::Null,
            }),
            "net.scan" => {
                let wifi = borea_net::nm::Wifi::system().map_err(|e| fail(e.to_string()))?;
                to_value(wifi.scan()?)
            }
            "net.status" => {
                let wifi = borea_net::nm::Wifi::system().map_err(|e| fail(e.to_string()))?;
                let _ = wifi.check_connectivity();
                let status = NetworkBackend::status(&wifi)?;
                let pending = self
                    .state
                    .lock()
                    .map_err(|_| fail("The network state is unavailable."))?
                    .pending_ssid
                    .clone();
                let connection = pending.map(|ssid| {
                    if wifi.connected_uuid(&ssid).ok().flatten().is_some() {
                        borea_net::ConnectionState::Connected
                    } else {
                        borea_net::connection_state(
                            &status,
                            &ssid,
                            wifi.activation_reason().ok().flatten(),
                        )
                    }
                });
                Ok(json!({"status":status,"connection":connection}))
            }
            "net.connect" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Connect {
                    ssid: String,
                    #[serde(default)]
                    hidden: bool,
                    secret: Option<borea_core::Secret>,
                }
                let p: Connect = params(input)?;
                let wifi = borea_net::nm::Wifi::system().map_err(|e| fail(e.to_string()))?;
                wifi.connect(
                    &p.ssid,
                    p.hidden,
                    p.secret.map(|s| borea_net::nm::Secret::new(s.expose().to_owned())),
                )?;
                self.state
                    .lock()
                    .map_err(|_| fail("The network state is unavailable."))?
                    .pending_ssid = Some(p.ssid);
                to_value(borea_net::ConnectionState::Connecting)
            }
            "disks.list" => to_value(borea_disk::probe()?),
            "plan.validate" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Validation {
                    plan: Value,
                    step: Option<borea_core::Step>,
                }
                let validation: Validation = params(input)?;
                if let Some(step) = validation.step {
                    validate_step(&validation.plan, step)?;
                    return Ok(Value::Null);
                }
                let p: PlanParams = params(input)?;
                p.plan.validate_intent()?;
                borea_locale::validate(&p.plan.locale)?;
                let hw = borea_catalog::probe::probe().map_err(|e| fail(e.to_string()))?;
                borea_core::review(&p.plan, &hw)?;
                borea_disk::validate(
                    &p.plan.disk,
                    &borea_disk::probe()?,
                    p.plan.desktop.is_some(),
                )?;
                Ok(Value::Null)
            }
            "plan.review" => {
                let p: PlanParams = params(input)?;
                let job = prepare(&p.plan)?;
                Ok(
                    json!({"review":job.review,"layout":job.layout,"bootstrap":job.bootstrap,"external":job.external,"approval":job.digest()?}),
                )
            }
            "plan.render" => {
                let p: PlanParams = params(input)?;
                let job = prepare(&p.plan)?;
                Ok(json!({"script":job.render_script(),"approval":job.digest()?}))
            }
            "install.start" => {
                let p: StartParams = params(input)?;
                p.secrets.validate(&p.plan.user)?;
                let job = prepare(&p.plan)?;
                let id = job.digest()?;
                if id != p.approval {
                    return Err(fail("The plan changed after Review; review it again."));
                }
                borea_net::check_mirror(borea_backend::mirror(p.plan.distro))?;
                let facts = borea_disk::probe()?;
                let (tx, rx) = mpsc::channel();
                {
                    let mut state = self
                        .state
                        .lock()
                        .map_err(|_| fail("The installation state is unavailable."))?;
                    if state.busy {
                        return Err(fail("An installation is already running."));
                    }
                    state.busy = true;
                    state.event_base += state.events.len();
                    state.events.clear();
                    state.fault = None;
                    state.worker = Some(tx);
                    state.job_id = Some(id.clone());
                }
                let state = Arc::clone(&self.state);
                let approval = id.clone();
                thread::spawn(move || {
                    let mut emit = |event: Event| {
                        if let Ok(mut s) = state.lock() {
                            s.push(event);
                        }
                    };
                    match Session::start(job, p.secrets, &approval, &facts) {
                        Ok(mut session) => {
                            let mut result = session.run(&mut emit);
                            if let Err(error) = &result {
                                if session.fault.is_none() {
                                    emit(Event::Failed {
                                        fault: borea_exec::Fault {
                                            action_id: "preflight".into(),
                                            title: error.message.clone(),
                                            command: "preflight".into(),
                                            exit_code: None,
                                            stderr_tail: vec![],
                                            suggestion: "Review the disk and connection again."
                                                .into(),
                                            retryable: false,
                                        },
                                    });
                                }
                            }
                            while result.is_err()
                                && session.fault.as_ref().is_some_and(|f| f.retryable)
                            {
                                match rx.recv() {
                                    Ok(action) => result = session.retry(&action, &mut emit),
                                    Err(_) => break,
                                }
                            }
                        }
                        Err(e) => emit(Event::Failed {
                            fault: borea_exec::Fault {
                                action_id: "preflight".into(),
                                title: e.message,
                                command: "preflight".into(),
                                exit_code: None,
                                stderr_tail: Vec::new(),
                                suggestion:
                                    "Correct the preflight error and review the plan again.".into(),
                                retryable: false,
                            },
                        }),
                    }
                    if let Ok(mut s) = state.lock() {
                        s.busy = false;
                        s.worker = None;
                    }
                });
                Ok(json!({"job_id":id}))
            }
            "install.events" => {
                #[derive(Deserialize, Default)]
                struct Events {
                    #[serde(default)]
                    after: usize,
                }
                let p: Events = if input == "null" { Events::default() } else { params(input)? };
                let s = self
                    .state
                    .lock()
                    .map_err(|_| fail("The installation state is unavailable."))?;
                s.events(p.after)
            }
            "install.retry" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Retry {
                    action_id: String,
                }
                let p: Retry = params(input)?;
                let s = self
                    .state
                    .lock()
                    .map_err(|_| fail("The installation state is unavailable."))?;
                if !s.fault.as_ref().is_some_and(|f| f.retryable && f.action_id == p.action_id) {
                    return Err(fail("This action cannot be retried."));
                }
                s.worker
                    .as_ref()
                    .ok_or_else(|| fail("There is no recoverable installation."))?
                    .send(p.action_id)
                    .map_err(|_| fail("The worker is no longer available."))?;
                Ok(Value::Null)
            }
            "install.log" => {
                Ok(json!({"log":fs::read_to_string("/run/borea/install.log").unwrap_or_default()}))
            }
            "system.reboot" => {
                if self
                    .state
                    .lock()
                    .map_err(|_| fail("The installation state is unavailable."))?
                    .busy
                {
                    return Err(fail("An installation is still running."));
                }
                let status = std::process::Command::new("/usr/bin/systemctl")
                    .arg("reboot")
                    .status()
                    .map_err(|e| fail(e.to_string()))?;
                if !status.success() {
                    return Err(fail("The reboot request failed."));
                }
                Ok(Value::Null)
            }
            _ => Err(fail("Unknown backend method.")),
        }
    }
    fn subscribe(&self, stream: &mut UnixStream, id: Value, raw: &str) -> Result<(), Hint> {
        #[derive(Deserialize, Default)]
        #[serde(deny_unknown_fields)]
        struct Subscription {
            #[serde(default)]
            after: usize,
        }
        let p = if raw == "null" { Subscription::default() } else { params(raw)? };
        let mut cursor = p.after;
        serde_json::to_writer(
            &mut *stream,
            &json!({"jsonrpc":"2.0","id":id,"result":{"subscribed":true}}),
        )
        .map_err(|e| fail(e.to_string()))?;
        stream.write_all(b"\n").map_err(|e| fail(e.to_string()))?;
        loop {
            let batch = self
                .state
                .lock()
                .map_err(|_| fail("The installation state is unavailable."))?
                .events(cursor)?;
            let next =
                batch["cursor"].as_u64().ok_or_else(|| fail("The event cursor is invalid."))?
                    as usize;
            let busy = batch["busy"] == true;
            if next != cursor || !busy {
                serde_json::to_writer(
                    &mut *stream,
                    &json!({"jsonrpc":"2.0","method":"install.events","params":batch}),
                )
                .map_err(|e| fail(e.to_string()))?;
                stream.write_all(b"\n").map_err(|e| fail(e.to_string()))?;
                cursor = next;
            }
            if !busy {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
    fn serve(&self, mut stream: UnixStream) -> Result<(), Hint> {
        let peer = getsockopt(&stream, PeerCredentials).map_err(|e| fail(e.to_string()))?;
        if peer.uid() != 0 && peer.uid() != self.allowed_uid {
            return Err(fail("This user is not authorized to control the installer."));
        }
        stream.set_read_timeout(Some(Duration::from_secs(30))).map_err(|e| fail(e.to_string()))?;
        stream.set_write_timeout(Some(Duration::from_secs(5))).map_err(|e| fail(e.to_string()))?;
        let reader = stream.try_clone().map_err(|e| fail(e.to_string()))?;
        let mut reader = BufReader::new(reader);
        loop {
            let mut bytes = Zeroizing::new(Vec::new());
            let n = reader
                .by_ref()
                .take(MAX_REQUEST + 1)
                .read_until(b'\n', &mut bytes)
                .map_err(|e| fail(e.to_string()))?;
            if n == 0 {
                return Ok(());
            }
            if n as u64 > MAX_REQUEST {
                return Err(fail("The request is too large."));
            }
            let request = serde_json::from_slice::<Request<'_>>(&bytes);
            let reply = match request {
                Ok(r) if r.jsonrpc == "2.0" => {
                    let raw = r.params.map_or("null", serde_json::value::RawValue::get);
                    if r.method == "install.subscribe" {
                        return self.subscribe(&mut stream, r.id, raw);
                    }
                    let result = self.call(&r.method, raw);
                    match result {
                        Ok(value) => json!({"jsonrpc":"2.0","id":r.id,"result":value}),
                        Err(error) => {
                            json!({"jsonrpc":"2.0","id":r.id,"error":{"code":-32000,"message":error.message,"data":{"field":error.field}}})
                        }
                    }
                }
                _ => {
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid JSON-RPC request."}})
                }
            };
            serde_json::to_writer(&mut stream, &reply).map_err(|e| fail(e.to_string()))?;
            stream.write_all(b"\n").map_err(|e| fail(e.to_string()))?;
        }
    }
}
fn run() -> Result<(), Hint> {
    if !Uid::effective().is_root() {
        return Err(fail("borea-installd must run as root."));
    }
    let user = User::from_name("borea")
        .map_err(|e| fail(e.to_string()))?
        .ok_or_else(|| fail("The live environment needs a borea user."))?;
    fs::create_dir_all("/run/borea").map_err(|e| fail(e.to_string()))?;
    fs::set_permissions("/run/borea", fs::Permissions::from_mode(0o750))
        .map_err(|e| fail(e.to_string()))?;
    if Path::new(SOCKET).exists() {
        return Err(fail(
            "The installer socket already exists; check the running service before restarting.",
        ));
    }
    nix::unistd::chown("/run/borea", None, Some(user.gid)).map_err(|e| fail(e.to_string()))?;
    let listener = UnixListener::bind(SOCKET).map_err(|e| fail(e.to_string()))?;
    nix::unistd::chown(SOCKET, None, Some(user.gid)).map_err(|e| fail(e.to_string()))?;
    fs::set_permissions(SOCKET, fs::Permissions::from_mode(0o660))
        .map_err(|e| fail(e.to_string()))?;
    let backend = Arc::new(Backend {
        state: Arc::new(Mutex::new(State::default())),
        allowed_uid: user.uid.as_raw(),
    });
    let active = Arc::new(AtomicUsize::new(0));
    for connection in listener.incoming() {
        let stream = connection.map_err(|e| fail(e.to_string()))?;
        if active.fetch_add(1, Ordering::Relaxed) >= 16 {
            active.fetch_sub(1, Ordering::Relaxed);
            continue;
        }
        let active = Arc::clone(&active);
        let backend = Arc::clone(&backend);
        thread::spawn(move || {
            let _ = backend.serve(stream);
            active.fetch_sub(1, Ordering::Relaxed);
        });
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--protocol") {
        print!("{}", protocol::markdown());
        return std::process::ExitCode::SUCCESS;
    }
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("borea-installd: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn event_cursors_remain_absolute_and_memory_is_bounded() {
        let mut state = State::default();
        for _ in 0..5000 {
            state.push(Event::Progress { fraction: 0.5 });
        }
        assert_eq!(state.events.len(), 4096);
        let all = state.events(0).unwrap_or_else(|_| unreachable!());
        assert_eq!(all["cursor"], 5000);
        assert_eq!(all["truncated"], true);
        let last = state.events(4999).unwrap_or_else(|_| unreachable!());
        assert_eq!(last["events"].as_array().map(Vec::len), Some(1));
        assert!(state.events(5001).is_err());
    }
    #[test]
    fn draft_validation_does_not_require_later_choices_or_choose_a_distro() {
        assert!(validate_step(&json!({"desktop":null}), borea_core::Step::Desktop).is_ok());
        assert!(validate_step(&json!({}), borea_core::Step::Desktop).is_err());
        assert!(validate_step(&json!({"release":"rolling"}), borea_core::Step::Distro).is_err());
        assert!(
            validate_step(&json!({"distro":"arch","release":"rolling"}), borea_core::Step::Distro)
                .is_ok()
        );
    }
    #[test]
    fn credentials_request_borrows_wire_buffer() {
        let raw = Zeroizing::new(String::from(
            r#"{"jsonrpc":"2.0","id":1,"method":"net.connect","params":{"ssid":"test","secret":"private-value"}}"#,
        ));
        let request: Request<'_> = serde_json::from_str(&raw).unwrap_or_else(|_| unreachable!());
        let params = request.params.unwrap_or_else(|| unreachable!()).get();
        assert!(params.as_ptr() >= raw.as_ptr());
        assert!(params.as_ptr() < raw.as_ptr().wrapping_add(raw.len()));
    }
    #[test]
    fn protocol_is_current() {
        assert_eq!(protocol::markdown(), include_str!("../../../docs/PROTOCOL.md"));
    }
}
