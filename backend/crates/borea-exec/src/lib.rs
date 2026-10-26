#![forbid(unsafe_code)]
//! Privileged execution. Only a freshly validated, approved Job may reach Session::start.
use borea_core::{Hint, Secrets, hint};
use borea_job::{Action, CommandSpec, Job, Root, SecretRef, TARGET};
use nix::{
    mount::{MntFlags, MsFlags, mount, umount2},
    sched::{CloneFlags, unshare},
    sys::signal::{Signal, killpg},
    unistd::{Pid, Uid},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum Event {
    StageStarted { stage: usize, title: String },
    Progress { fraction: f32 },
    Line { stderr: bool, text: String },
    ActionDone { id: String },
    Failed { fault: Fault },
    Finished,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fault {
    pub action_id: String,
    pub title: String,
    pub command: String,
    pub exit_code: Option<i32>,
    pub stderr_tail: Vec<String>,
    pub suggestion: String,
    pub retryable: bool,
}
#[derive(Clone)]
pub struct Redactor {
    values: Arc<Vec<Zeroizing<String>>>,
}
impl Redactor {
    pub fn new(secrets: &Secrets) -> Self {
        let mut values = vec![Zeroizing::new(secrets.passphrase.expose().to_owned())];
        if let Some(s) = &secrets.admin {
            values.push(Zeroizing::new(s.expose().to_owned()));
        }
        if let Some(s) = &secrets.wifi {
            values.push(Zeroizing::new(s.expose().to_owned()));
        }
        values.sort_by_key(|s| std::cmp::Reverse(s.len()));
        Self { values: Arc::new(values) }
    }
    pub fn redact(&self, text: &str) -> String {
        let mut s = text.to_owned();
        for value in self.values.iter() {
            if !value.is_empty() {
                s = s.replace(value.as_str(), "[REDACTED]");
            }
        }
        s
    }
}
fn err(message: impl Into<String>) -> Hint {
    hint("execution", message)
}
fn io<T>(r: std::io::Result<T>) -> Result<T, Hint> {
    r.map_err(|e| err(e.to_string()))
}
/// Reap the entire command group even when stdin, logging or output handling fails.
struct ChildGuard(Child);
impl std::ops::Deref for ChildGuard {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.0
    }
}
impl std::ops::DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL);
            let _ = self.0.wait();
        }
    }
}
/// This session must remain on its owning thread (namespace and mounted state).
pub struct Session {
    pub job: Job,
    secrets: Secrets,
    redactor: Redactor,
    vars: BTreeMap<String, String>,
    mounted: Vec<PathBuf>,
    log: File,
    pub stage: usize,
    pub action: usize,
    pub fault: Option<Fault>,
    started_writes: bool,
    finished: bool,
    deferred: Vec<postinstall::Deferred>,
    opened_luks: bool,
    command_failure: Option<(Option<i32>, Vec<String>)>,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl Session {
    pub fn start(
        job: Job,
        secrets: Secrets,
        approved: &str,
        facts: &borea_disk::DiskFacts,
    ) -> Result<Self, Hint> {
        if !Uid::effective().is_root() {
            return Err(err("Installation requires the privileged service."));
        }
        if job.digest()? != approved {
            return Err(err("The installation plan changed after Review."));
        }
        if Path::new(&format!("/dev/mapper/borea-{}", &approved[..16])).exists() {
            return Err(err(
                "This plan's encrypted mapping is already in use; inspect the previous session before retrying.",
            ));
        }
        let hw = borea_catalog::probe::probe().map_err(|e| err(e.to_string()))?;
        if hw.secure_boot != job.secure_boot || hw.setup_mode != job.setup_mode {
            return Err(err("Firmware trust state changed after Review."));
        }
        if job.plan.distro == borea_core::Distribution::Arch
            && hw.secure_boot
            && hw.setup_mode != Some(true)
        {
            return Err(err(
                "Arch Secure Boot enrollment requires firmware Setup Mode; enter Setup Mode or disable Secure Boot before installing.",
            ));
        }
        let mut regenerated = borea_job::compile(&job.plan, &hw, facts, &job.bootstrap)?;
        borea_job::apply_external(&mut regenerated, job.external.clone());
        if regenerated.digest()? != approved {
            return Err(err(
                "The executable job no longer matches the current facts and compiler.",
            ));
        }
        secrets.validate(&job.plan.user)?;
        borea_disk::validate(&job.plan.disk, facts, job.plan.desktop.is_some())?;
        unshare(CloneFlags::CLONE_NEWNS)
            .map_err(|e| err(format!("Cannot isolate installation mounts: {e}")))?;
        mount(None::<&str>, "/", None::<&str>, MsFlags::MS_REC | MsFlags::MS_PRIVATE, None::<&str>)
            .map_err(|e| err(e.to_string()))?;
        io(fs::create_dir_all("/run/borea"))?;
        io(fs::set_permissions("/run/borea", fs::Permissions::from_mode(0o750)))?;
        let log = io(OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open("/run/borea/install.log"))?;
        let redactor = Redactor::new(&secrets);
        Ok(Self {
            job,
            secrets,
            redactor,
            vars: BTreeMap::from([("MAPPER_NAME".into(), format!("borea-{}", &approved[..16]))]),
            mounted: Vec::new(),
            log,
            stage: 0,
            action: 0,
            fault: None,
            started_writes: false,
            finished: false,
            deferred: Vec::new(),
            opened_luks: false,
            command_failure: None,
            _thread_bound: std::marker::PhantomData,
        })
    }
    pub fn run(&mut self, emit: &mut impl FnMut(Event)) -> Result<(), Hint> {
        self.fault = None;
        while self.stage < self.job.stages.len() {
            let stage = self.job.stages[self.stage].clone();
            emit(Event::StageStarted { stage: self.stage, title: stage.title.clone() });
            while self.action < stage.actions.len() {
                let id = format!("{}.{}", self.stage, self.action);
                let action = stage.actions[self.action].clone();
                if self.stage == 1 && self.action == 0 {
                    let facts = borea_disk::probe()?;
                    borea_disk::validate(
                        &self.job.plan.disk,
                        &facts,
                        self.job.plan.desktop.is_some(),
                    )?;
                    if borea_disk::layout(
                        &self.job.plan.disk,
                        &facts,
                        self.job.plan.distro,
                        self.job.plan.desktop.is_some(),
                    )? != self.job.layout
                    {
                        return Err(err(
                            "The disk layout changed while preparing downloads; review the disk again.",
                        ));
                    }

                    self.started_writes = true;
                }
                if let Err(error) = self.execute(&action, emit) {
                    let retryable = matches!(&action,Action::Run(c) if c.retry && ["/usr/bin/apt-get","/usr/bin/dnf","/usr/bin/pacman","/usr/bin/zypper","/usr/bin/flatpak","/usr/sbin/akmods"].contains(&c.program.as_str()))
                        && self.stage > 2;
                    let (exit_code, stderr_tail) = self.command_failure.take().unwrap_or_default();
                    let fault = Fault {
                        action_id: id,
                        title: error.message.clone(),
                        command: self.redactor.redact(&format!("{action:?}")),
                        exit_code,
                        stderr_tail,
                        suggestion: if error.message.contains("signature")
                            || error.message.contains("key")
                        {
                            "Check the upstream signing key and retry verification."
                        } else {
                            "Check the connection and available space; inspect the redacted log."
                        }
                        .into(),
                        retryable,
                    };
                    emit(Event::Failed { fault: fault.clone() });
                    self.fault = Some(fault);
                    return Err(error);
                }
                emit(Event::ActionDone { id });
                self.action += 1;
                let done: u32 = self.job.stages.iter().take(self.stage).map(|s| s.weight).sum();
                emit(Event::Progress {
                    fraction: (done as f32
                        + stage.weight as f32 * self.action as f32
                            / stage.actions.len().max(1) as f32)
                        / 100.0,
                });
            }
            self.stage += 1;
            self.action = 0;
        }
        self.copy_log()?;
        self.cleanup()?;
        self.finished = true;
        emit(Event::Finished);
        Ok(())
    }
    pub fn retry(&mut self, id: &str, emit: &mut impl FnMut(Event)) -> Result<(), Hint> {
        let fault =
            self.fault.as_ref().ok_or_else(|| err("There is no failed action to retry."))?;
        if fault.action_id != id || !fault.retryable {
            return Err(err("This action cannot be retried automatically."));
        }
        self.run(emit)
    }
    fn expand(&self, s: &str) -> Result<String, Hint> {
        let mut result = s.to_owned();
        for (k, v) in &self.vars {
            result = result.replace(&format!("${{{k}}}"), v);
        }
        if result.contains("${") {
            return Err(err("A required runtime identifier has not been discovered."));
        }
        Ok(result)
    }
    fn target_path(&self, path: &str) -> Result<PathBuf, Hint> {
        let rel =
            Path::new(path).strip_prefix("/").map_err(|_| err("Target paths must be absolute."))?;
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(err("Target path escapes its root."));
        }
        let root = Path::new(TARGET);
        let target = root.join(rel);
        let mut current = root.to_path_buf();
        for part in rel.components() {
            current.push(part);
            if io(fs::symlink_metadata(&current).or_else(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    fs::symlink_metadata(root)
                } else {
                    Err(e)
                }
            }))?
            .file_type()
            .is_symlink()
            {
                return Err(err(format!("Refusing symlink in target path {}.", current.display())));
            }
        }
        Ok(target)
    }
    fn write(&self, path: &str, contents: &str, mode: u32) -> Result<(), Hint> {
        let path = self.target_path(path)?;
        let parent = path.parent().ok_or_else(|| err("Target file has no directory."))?;
        io(fs::create_dir_all(parent))?;
        let mut f = io(OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(mode)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&path))?;
        io(f.set_permissions(fs::Permissions::from_mode(mode)))?;
        io(f.write_all(self.expand(contents)?.as_bytes()))?;
        io(f.sync_all())
    }
    fn cmd(&mut self, c: &CommandSpec, emit: &mut impl FnMut(Event)) -> Result<String, Hint> {
        self.command_failure = None;
        if !c.program.starts_with('/') || c.program.contains("/../") {
            return Err(err("Commands must use an absolute executable path."));
        }
        if c.root == Root::Tools && self.mounted.iter().any(|p| p == Path::new(TARGET)) {
            let destination =
                PathBuf::from(format!("/run/borea/tools/{}/mnt", self.job.plan.distro.key()));
            if !self.mounted.contains(&destination) {
                io(fs::create_dir_all(&destination))?;
                mount(
                    Some(TARGET),
                    &destination,
                    None::<&str>,
                    MsFlags::MS_BIND | MsFlags::MS_REC,
                    None::<&str>,
                )
                .map_err(|e| err(e.to_string()))?;
                self.mounted.push(destination);
            }
        }
        let mut args = c.args.iter().map(|s| self.expand(s)).collect::<Result<Vec<_>, _>>()?;
        let ca = if c.root != Root::Live {
            let root = match c.root {
                Root::Target => PathBuf::from(TARGET),
                Root::Tools => {
                    PathBuf::from(format!("/run/borea/tools/{}", self.job.plan.distro.key()))
                }
                Root::Live => PathBuf::from("/"),
            };
            io(fs::create_dir_all(root.join("run/borea")))?;
            let bundle = std::env::var_os("SSL_CERT_FILE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/etc/ssl/certs/ca-certificates.crt"));
            io(fs::copy(bundle, root.join("run/borea/session-ca.pem")))?;
            Some("/run/borea/session-ca.pem")
        } else {
            None
        };
        if let Some(ca) = ca {
            if c.program == "/usr/bin/apt-get" {
                args.splice(0..0, ["-o".into(), format!("Acquire::https::CaInfo={ca}")]);
            } else if c.program == "/usr/bin/dnf" {
                args.insert(0, format!("--setopt=sslcacert={ca}"));
            }
        }
        let mut command = match c.root {
            Root::Live => Command::new(&c.program),
            Root::Target => {
                let mut cmd = Command::new("/usr/sbin/chroot");
                cmd.arg(TARGET).arg(&c.program);
                cmd
            }
            Root::Tools => {
                let mut cmd = Command::new("/usr/sbin/chroot");
                cmd.arg(format!("/run/borea/tools/{}", self.job.plan.distro.key())).arg(&c.program);
                cmd
            }
        };
        command
            .args(&args)
            .env_clear()
            .env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
            .env("HOME", "/root")
            .env("LC_ALL", "C.UTF-8")
            .env("DEBIAN_FRONTEND", "noninteractive")
            .env("SYSTEMD_OFFLINE", "1")
            .process_group(0);
        // Proxy/CA trust is operational configuration, not part of the reviewed command.
        for name in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "no_proxy",
            "SSL_CERT_FILE",
        ] {
            if let Ok(value) = std::env::var(name) {
                command.env(name, value);
            }
        }
        if let Some(ca) = ca {
            for name in ["SSL_CERT_FILE", "CURL_CA_BUNDLE", "REQUESTS_CA_BUNDLE", "GIT_SSL_CAINFO"]
            {
                command.env(name, ca);
            }
        }
        let line = self.redactor.redact(&format!(
            "$ {} {}",
            c.program,
            args.iter().map(|s| borea_job::quote(s)).collect::<Vec<_>>().join(" ")
        ));
        io(writeln!(self.log, "{line}"))?;
        emit(Event::Line { stderr: false, text: line });
        let mut child = ChildGuard(io(command
            .stdin(if c.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn())?);
        if let Some(which) = &c.stdin {
            let input = match which {
                SecretRef::Luks => Zeroizing::new(self.secrets.passphrase.expose().to_owned()),
                SecretRef::Login => Zeroizing::new(format!(
                    "{}:{}\n",
                    self.job.plan.user.username,
                    self.secrets.passphrase.expose()
                )),
                SecretRef::RootPassword => Zeroizing::new(format!(
                    "root:{}\n",
                    if self.job.plan.user.same_passphrase {
                        self.secrets.passphrase.expose()
                    } else {
                        self.secrets
                            .admin
                            .as_ref()
                            .ok_or_else(|| err("The administrator passphrase is missing."))?
                            .expose()
                    }
                )),
                SecretRef::Mok => Zeroizing::new(format!(
                    "{}\n{}\n",
                    self.secrets.passphrase.expose(),
                    self.secrets.passphrase.expose()
                )),
            };
            let mut stdin =
                child.stdin.take().ok_or_else(|| err("A secret input pipe was not created."))?;
            io(stdin.write_all(input.as_bytes()))?;
        }
        let stdout =
            child.stdout.take().ok_or_else(|| err("The command output pipe is missing."))?;
        let stderr =
            child.stderr.take().ok_or_else(|| err("The command error pipe is missing."))?;
        let (tx, rx) = mpsc::sync_channel::<(bool, Zeroizing<String>)>(128);
        let readers = [
            (Box::new(stdout) as Box<dyn Read + Send>, false),
            (Box::new(stderr) as Box<dyn Read + Send>, true),
        ]
        .into_iter()
        .map(|(pipe, is_err)| {
            let tx = tx.clone();
            thread::spawn(move || {
                let mut reader = BufReader::new(pipe);
                loop {
                    let mut bytes = Zeroizing::new(Vec::new());
                    let n = match reader.by_ref().take(65537).read_until(b'\n', &mut bytes) {
                        Ok(n) => n,
                        Err(_) => break,
                    };
                    if n == 0 {
                        break;
                    }
                    if bytes.len() > 65536 && !bytes.ends_with(b"\n") {
                        loop {
                            bytes.clear();
                            match reader.by_ref().take(65537).read_until(b'\n', &mut bytes) {
                                Ok(0) | Err(_) => break,
                                Ok(_) => {
                                    if bytes.ends_with(b"\n") {
                                        break;
                                    }
                                }
                            }
                        }
                        bytes.clear();
                        bytes.extend_from_slice(b"[Overlong command output omitted]\n");
                    }
                    if tx
                        .send((
                            is_err,
                            Zeroizing::new(String::from_utf8_lossy(&bytes).into_owned()),
                        ))
                        .is_err()
                    {
                        break;
                    }
                }
            })
        })
        .collect::<Vec<_>>();
        drop(tx);
        let start = Instant::now();
        let mut output = String::new();
        let mut tail = VecDeque::new();
        let mut status = None;
        loop {
            match rx.recv_timeout(Duration::from_millis(25)) {
                Ok((is_err, line)) => {
                    let text = self.redactor.redact(line.trim_end());
                    io(writeln!(self.log, "{text}"))?;
                    emit(Event::Line { stderr: is_err, text: text.clone() });
                    if is_err {
                        tail.push_back(text.clone());
                        if tail.len() > 40 {
                            tail.pop_front();
                        }
                    } else if output.len() < 1024 * 1024 {
                        output.push_str(&text);
                        output.push('\n');
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if status.is_none() {
                        status = io(child.try_wait())?;
                    }
                    if status.is_some() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(25));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if status.is_none() {
                status = io(child.try_wait())?;
            }
            if start.elapsed() > Duration::from_secs(c.timeout) {
                let pid = Pid::from_raw(child.id() as i32);
                let _ = killpg(pid, Signal::SIGKILL);
                let _ = child.wait();
                for r in readers {
                    let _ = r.join();
                }
                self.command_failure = Some((None, tail.into_iter().collect()));
                return Err(err("The command exceeded its time limit."));
            }
        }
        for r in readers {
            r.join().map_err(|_| err("The command output reader failed."))?;
        }
        let status = status.ok_or_else(|| err("The command exited without a status."))?;
        if !status.success() {
            self.command_failure = Some((status.code(), tail.iter().cloned().collect()));
            return Err(err(format!(
                "{} failed (exit {:?}): {}",
                c.program,
                status.code(),
                tail.into_iter().collect::<Vec<_>>().join("\n")
            )));
        }
        if c.root == Root::Live
            && c.program == "/usr/sbin/cryptsetup"
            && c.args.first().is_some_and(|s| s == "open")
        {
            self.opened_luks = true;
        }
        if let Some(key) = &c.capture {
            let value = output.trim();
            if value.is_empty()
                || value.len() > 256
                || !value.bytes().all(|c| c.is_ascii_alphanumeric() || b"-._+".contains(&c))
            {
                return Err(err("A discovered identifier is invalid or ambiguous."));
            }
            self.vars.insert(key.clone(), value.into());
        }
        Ok(output)
    }
    fn command(
        &mut self,
        root: Root,
        program: &str,
        args: Vec<String>,
        emit: &mut impl FnMut(Event),
    ) -> Result<String, Hint> {
        self.cmd(
            &CommandSpec {
                program: program.into(),
                args,
                root,
                stdin: None,
                capture: None,
                timeout: 3600,
                retry: true,
            },
            emit,
        )
    }
    fn execute(&mut self, a: &Action, emit: &mut impl FnMut(Event)) -> Result<(), Hint> {
        match a {
            Action::Run(c) => {
                if let Err(error) = self.cmd(c, emit) {
                    let text = error.message.to_ascii_lowercase();
                    if c.root == Root::Target
                        && c.program == "/usr/bin/flatpak"
                        && c.args.first().is_some_and(|s| s == "install")
                        && (text.contains("dbus")
                            || text.contains("d-bus")
                            || text.contains("system helper"))
                    {
                        self.deferred.push(postinstall::Deferred {
                            program: c.program.clone(),
                            args: c.args.clone(),
                        });
                        self.write(
                            "/var/lib/borea/firstboot.json",
                            &serde_json::to_string(&self.deferred)
                                .map_err(|e| err(e.to_string()))?,
                            0o600,
                        )?;
                        self.command(
                            Root::Target,
                            "/usr/bin/systemctl",
                            vec!["enable".into(), "borea-firstboot.service".into()],
                            emit,
                        )?;
                        emit(Event::Line {
                            stderr: false,
                            text:
                                "Flatpak requires the installed system bus; queued for first boot."
                                    .into(),
                        });
                    } else {
                        return Err(error);
                    }
                }
            }
            Action::WriteFile { path, contents, mode } => self.write(path, contents, *mode)?,
            Action::Mount { source, path, fs, options } => {
                if !Path::new(path).starts_with(TARGET) {
                    return Err(err("A mount destination is outside the private target."));
                }
                io(fs::create_dir_all(path))?;
                let source = self.expand(source)?;
                let flags = if options.contains("rbind") {
                    MsFlags::MS_BIND | MsFlags::MS_REC
                } else {
                    MsFlags::empty()
                };
                let data = if flags.is_empty() { Some(options.as_str()) } else { None };
                mount(Some(source.as_str()), path.as_str(), fs.as_deref(), flags, data)
                    .map_err(|e| err(format!("Mounting {path} failed: {e}")))?;
                self.mounted.push(path.into());
                mount(
                    None::<&str>,
                    path.as_str(),
                    None::<&str>,
                    MsFlags::MS_SLAVE | MsFlags::MS_REC,
                    None::<&str>,
                )
                .map_err(|e| err(e.to_string()))?;
            }
            Action::Unmount { path } => {
                umount2(path.as_str(), MntFlags::empty()).map_err(|e| err(e.to_string()))?;
                self.mounted.retain(|p| p != Path::new(path));
            }
            Action::AcquireToolRoot { artifact } => self.acquire(artifact, emit)?,
            Action::PrepareExternal => self.prepare_external()?,
            Action::PreflightNative => self.preflight_native(emit)?,
            Action::CopyNativeRepositories => {
                let root =
                    PathBuf::from(format!("/run/borea/tools/{}", self.job.plan.distro.key()));
                for entry in io(fs::read_dir(root.join("etc/zypp/repos.d")))? {
                    let entry = io(entry)?;
                    if io(entry.file_type())?.is_file() {
                        let content = io(fs::read_to_string(entry.path()))?;
                        self.write(
                            &format!("/etc/zypp/repos.d/{}", entry.file_name().to_string_lossy()),
                            &content,
                            0o644,
                        )?;
                    }
                }
            }
            Action::ImportRepositoryKeys => {
                for key in self.job.external.repository_keys.clone() {
                    let live = Path::new("/run/borea/external").join(format!("{}.asc", key.source));
                    let target = format!("/etc/pki/rpm-gpg/BOREA-{}", key.source);
                    let content = io(fs::read_to_string(live))?;
                    self.write(&target, &content, 0o644)?;
                    self.command(
                        Root::Target,
                        "/usr/bin/rpm",
                        vec!["--import".into(), target],
                        emit,
                    )?;
                }
            }
            Action::InstallHelpers => self.install_helpers()?,
            Action::ConfigureFstab => self.fstab(emit)?,
            Action::CopyNetwork => self.copy_network()?,
            Action::BuildAur { packages } => self.aur(packages, emit)?,
            Action::InstallAppImages { releases } => self.appimages(releases, emit)?,
            Action::Note { message } => emit(Event::Line { stderr: false, text: message.clone() }),
        }
        Ok(())
    }
    fn acquire(
        &mut self,
        a: &borea_backend::Artifact,
        emit: &mut impl FnMut(Event),
    ) -> Result<(), Hint> {
        let dir = PathBuf::from(format!("/run/borea/tools/{}", self.job.plan.distro.key()));
        if dir.exists() {
            return Err(err(
                "A native tool root already exists; clean up the previous session first.",
            ));
        }
        io(fs::create_dir_all(&dir))?;
        io(fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)))?;
        let archive = dir.with_extension("tar");
        let signature = dir.with_extension("sig");
        download(&a.url, &archive, Some(&a.sha256))?;
        download(&a.signature_url, &signature, None)?;
        if let Some(proof) = &a.checksum {
            if !borea_backend::external::identifier(&proof.filename) {
                return Err(err("Invalid signed checksum filename."));
            }
            let metadata = dir.with_extension("checksums");
            download(&proof.url, &metadata, None)?;
            let verified = dir.with_extension("verified-checksums");
            if proof.url == a.signature_url {
                self.command(
                    Root::Live,
                    "/usr/bin/gpgv",
                    vec![
                        "--keyring".into(),
                        a.keyring.clone(),
                        "--output".into(),
                        verified.display().to_string(),
                        metadata.display().to_string(),
                    ],
                    emit,
                )?;
            } else {
                self.command(
                    Root::Live,
                    "/usr/bin/gpgv",
                    vec![
                        "--keyring".into(),
                        a.keyring.clone(),
                        signature.display().to_string(),
                        metadata.display().to_string(),
                    ],
                    emit,
                )?;
                io(fs::copy(&metadata, &verified))?;
            }
            let sums = io(fs::read_to_string(&verified))?;
            if !borea_backend::bootstrap::checksum_matches(&sums, &proof.filename, &a.sha256) {
                return Err(err(
                    "The artifact hash is not present in the authenticated checksum metadata.",
                ));
            }
            io(fs::remove_file(metadata))?;
            io(fs::remove_file(verified))?;
        } else {
            self.command(
                Root::Live,
                "/usr/bin/gpgv",
                vec![
                    "--keyring".into(),
                    a.keyring.clone(),
                    signature.display().to_string(),
                    archive.display().to_string(),
                ],
                emit,
            )?;
        }
        let listing = self.command(
            Root::Live,
            "/usr/bin/tar",
            vec!["-tf".into(), archive.display().to_string()],
            emit,
        )?;
        if listing.lines().any(|s| {
            Path::new(s).is_absolute()
                || Path::new(s).components().any(|c| matches!(c, Component::ParentDir))
        }) {
            return Err(err("The tool archive contains an unsafe path."));
        }
        let docker = listing.lines().any(|s| s.trim_start_matches("./") == "manifest.json");
        let oci = listing.lines().any(|s| s.trim_start_matches("./") == "oci-layout");
        if docker || oci {
            let layout = dir.with_extension("oci");
            let bundle = dir.with_extension("bundle");
            self.command(
                Root::Live,
                "/usr/bin/skopeo",
                vec![
                    "copy".into(),
                    format!(
                        "{}:{}",
                        if docker { "docker-archive" } else { "oci-archive" },
                        archive.display()
                    ),
                    format!("oci:{}:borea", layout.display()),
                ],
                emit,
            )?;
            self.command(
                Root::Live,
                "/usr/bin/umoci",
                vec![
                    "unpack".into(),
                    "--image".into(),
                    format!("{}:borea", layout.display()),
                    bundle.display().to_string(),
                ],
                emit,
            )?;
            io(fs::remove_dir(&dir))?;
            io(fs::rename(bundle.join("rootfs"), &dir))?;
            io(fs::remove_dir_all(bundle))?;
            io(fs::remove_dir_all(layout))?;
        } else {
            let strip =
                if self.job.plan.distro == borea_core::Distribution::Arch { "1" } else { "0" };
            self.command(
                Root::Live,
                "/usr/bin/tar",
                vec![
                    "--extract".into(),
                    "--file".into(),
                    archive.display().to_string(),
                    "--directory".into(),
                    dir.display().to_string(),
                    format!("--strip-components={strip}"),
                ],
                emit,
            )?;
        }
        for name in ["proc", "sys", "dev"] {
            let target = dir.join(name);
            io(fs::create_dir_all(&target))?;
            let (src, typ, flags) = if name == "proc" {
                ("proc".into(), Some("proc"), MsFlags::empty())
            } else {
                (format!("/{name}"), None, MsFlags::MS_BIND | MsFlags::MS_REC)
            };
            mount(Some(src.as_str()), &target, typ, flags, None::<&str>)
                .map_err(|e| err(e.to_string()))?;
            self.mounted.push(target);
        }
        let tool = match self.job.plan.distro {
            borea_core::Distribution::Arch => "/usr/bin/pacstrap",
            borea_core::Distribution::Fedora => "/usr/bin/dnf",
            borea_core::Distribution::OpenSuse => "/usr/bin/zypper",
            borea_core::Distribution::Debian => "/usr/sbin/debootstrap",
        };
        if !dir.join(tool.trim_start_matches('/')).exists() {
            return Err(err(
                "The verified archive does not contain the required native package tooling.",
            ));
        }
        // /mnt is bound lazily once the selected target has been mounted.
        io(fs::remove_file(archive))?;
        io(fs::remove_file(signature))?;
        Ok(())
    }
    fn preflight_native(&mut self, emit: &mut impl FnMut(Event)) -> Result<(), Hint> {
        use borea_core::Distribution as D;
        let distro = self.job.plan.distro;
        let root = PathBuf::from(format!("/run/borea/tools/{}", distro.key()));
        let resolv = root.join("etc/resolv.conf");
        if resolv.is_symlink() {
            io(fs::remove_file(&resolv))?;
        }
        io(fs::copy("/etc/resolv.conf", resolv))?;
        let sources = self
            .job
            .review
            .packages
            .iter()
            .map(|p| p.source.clone())
            .chain(self.job.plan.sources.iter().filter(|(_, on)| **on).map(|(key, _)| key.clone()))
            .collect::<std::collections::BTreeSet<_>>();
        for key in self.job.external.repository_keys.clone() {
            let target = root.join(format!("etc/pki/rpm-gpg/BOREA-{}", key.source));
            io(fs::create_dir_all(target.parent().ok_or_else(|| err("Missing key directory."))?))?;
            io(fs::copy(format!("/run/borea/external/{}.asc", key.source), &target))?;
            self.command(
                Root::Tools,
                "/usr/bin/rpm",
                vec!["--import".into(), format!("/etc/pki/rpm-gpg/BOREA-{}", key.source)],
                emit,
            )?;
        }
        match distro {
            D::Arch => {
                io(fs::write(
                    root.join("etc/pacman.d/mirrorlist"),
                    "Server = https://geo.mirror.pkgbuild.com/$repo/os/$arch\n",
                ))?;
                let multilib = if sources.contains("arch.multilib") {
                    "[multilib]\nInclude = /etc/pacman.d/mirrorlist\n"
                } else {
                    ""
                };
                io(fs::write(
                    root.join("etc/pacman.conf"),
                    format!(
                        "[options]\nArchitecture = auto\nSigLevel = Required DatabaseOptional\n[core]\nInclude = /etc/pacman.d/mirrorlist\n[extra]\nInclude = /etc/pacman.d/mirrorlist\n{multilib}"
                    ),
                ))?;
                self.command(Root::Tools, "/usr/bin/pacman-key", vec!["--init".into()], emit)?;
                self.command(
                    Root::Tools,
                    "/usr/bin/pacman-key",
                    vec!["--populate".into(), "archlinux".into()],
                    emit,
                )?;
                self.command(
                    Root::Tools,
                    "/usr/bin/pacman",
                    vec!["-Sy".into(), "--noconfirm".into()],
                    emit,
                )?;
            }
            D::Fedora => {
                for kind in ["free", "nonfree"] {
                    if sources.contains(&format!("fedora.rpmfusion-{kind}")) {
                        self.command(Root::Tools,"/usr/bin/dnf",vec!["--setopt=localpkg_gpgcheck=1".into(),"install".into(),"-y".into(),format!("https://download1.rpmfusion.org/{kind}/fedora/rpmfusion-{kind}-release-{}.noarch.rpm",self.job.plan.release)],emit)?;
                    }
                }
                self.command(
                    Root::Tools,
                    "/usr/bin/dnf",
                    vec!["makecache".into(), "--refresh".into()],
                    emit,
                )?;
            }
            D::OpenSuse => {
                if sources.contains("opensuse.nvidia") {
                    self.command(
                        Root::Tools,
                        "/usr/bin/zypper",
                        vec![
                            "--non-interactive".into(),
                            "install".into(),
                            "openSUSE-repos-NVIDIA".into(),
                        ],
                        emit,
                    )?;
                }
                if sources.contains("opensuse.packman") {
                    self.command(
                        Root::Tools,
                        "/usr/bin/zypper",
                        [
                            "--non-interactive",
                            "addrepo",
                            "--refresh",
                            "--priority",
                            "90",
                            "https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Tumbleweed/",
                            "packman",
                        ]
                        .map(String::from)
                        .to_vec(),
                        emit,
                    )?;
                }
                self.command(
                    Root::Tools,
                    "/usr/bin/zypper",
                    vec!["--non-interactive".into(), "refresh".into()],
                    emit,
                )?;
            }
            D::Debian => return Ok(()),
        }
        let packages = self
            .job
            .review
            .packages
            .iter()
            .filter(|p| p.source.contains('.'))
            .map(|p| p.name.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        match distro {
            D::Arch => {
                self.command(
                    Root::Tools,
                    "/usr/bin/pacman",
                    vec!["-Sp".into(), "--noconfirm".into()].into_iter().chain(packages).collect(),
                    emit,
                )?;
            }
            D::Fedora => {
                let found = self.command(
                    Root::Tools,
                    "/usr/bin/dnf",
                    vec!["repoquery".into(), "--available".into(), "--queryformat=%{name}".into()]
                        .into_iter()
                        .chain(packages.iter().cloned())
                        .collect(),
                    emit,
                )?;
                for package in packages {
                    if package.starts_with('@') {
                        let name = package.trim_start_matches('@').trim_start_matches('^');
                        self.command(
                            Root::Tools,
                            "/usr/bin/dnf",
                            vec!["environment".into(), "info".into(), name.into()],
                            emit,
                        )?;
                        continue;
                    }
                    if !found.lines().any(|line| line.trim() == package) {
                        return Err(err(format!(
                            "The Fedora package {package} is unavailable; report this catalog entry."
                        )));
                    }
                }
            }
            D::OpenSuse => {
                self.command(
                    Root::Tools,
                    "/usr/bin/zypper",
                    vec![
                        "--non-interactive".into(),
                        "info".into(),
                        "--type".into(),
                        "package".into(),
                    ]
                    .into_iter()
                    .chain(packages)
                    .collect(),
                    emit,
                )?;
            }
            D::Debian => {}
        }
        Ok(())
    }
    fn fstab(&mut self, emit: &mut impl FnMut(Event)) -> Result<(), Hint> {
        let layout = self.job.layout.clone();
        let mut text = String::new();
        for (sub, path) in &layout.subvolumes {
            text.push_str(&format!(
                "UUID={} {path} btrfs subvol={sub},compress=zstd:1,noatime 0 0\n",
                self.vars.get("ROOT_UUID").ok_or_else(|| err("The root UUID is unavailable."))?
            ));
        }
        for (device, path, fs, options) in
            std::iter::once((layout.esp.clone(), layout.esp_mount.clone(), "vfat", "umask=0077"))
                .chain(
                    layout
                        .boot
                        .clone()
                        .map(|p| (p, "/boot".into(), layout.boot_fs.as_str(), "defaults")),
                )
        {
            let uuid = self.command(
                Root::Live,
                "/usr/sbin/blkid",
                vec![
                    "-s".into(),
                    "UUID".into(),
                    "-o".into(),
                    "value".into(),
                    device.display().to_string(),
                ],
                emit,
            )?;
            let uuid = uuid.trim();
            if uuid.is_empty() || !uuid.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
                return Err(err("A filesystem UUID was not valid."));
            }
            if path == "/boot" {
                self.vars.insert("BOOT_UUID".into(), uuid.into());
            }
            text.push_str(&format!("UUID={uuid} {path} {fs} {options} 0 2\n"));
        }
        self.write("/etc/fstab", &text, 0o644)?;
        self.write(
            "/etc/crypttab",
            &format!(
                "borea-root UUID={} none luks,discard\n",
                self.vars.get("LUKS_UUID").ok_or_else(|| err("The LUKS UUID is unavailable."))?
            ),
            0o600,
        )
    }
    fn copy_network(&self) -> Result<(), Hint> {
        let selected = match &self.job.plan.net {
            borea_core::NetChoice::Ethernet => None,
            borea_core::NetChoice::Wifi { ssid } | borea_core::NetChoice::Hidden { ssid } => {
                Some(ssid)
            }
        };
        if let Some(ssid) = selected {
            let wifi = borea_net::nm::Wifi::system().map_err(|e| err(e.to_string()))?;
            let uuid = wifi
                .connected_uuid(ssid)
                .map_err(|e| err(e.to_string()))?
                .ok_or_else(|| err("The selected Wi-Fi network is no longer connected."))?;
            let entries = io(fs::read_dir("/etc/NetworkManager/system-connections"))?;
            let mut copied = false;
            for entry in entries {
                let entry = io(entry)?;
                if !io(entry.file_type())?.is_file() {
                    continue;
                }
                let content = Zeroizing::new(io(fs::read_to_string(entry.path()))?);
                if content.lines().any(|l| l.strip_prefix("uuid=") == Some(uuid.as_str())) {
                    self.write(
                        &format!(
                            "/etc/NetworkManager/system-connections/{}",
                            entry.file_name().to_string_lossy()
                        ),
                        &content,
                        0o600,
                    )?;
                    copied = true;
                }
            }
            if !copied {
                return Err(err(
                    "The selected Wi-Fi connection could not be carried into the target.",
                ));
            }
        }
        // DNS resolution inside native chroots, without following a target symlink.
        let content = io(fs::read_to_string("/etc/resolv.conf"))?;
        let path = Path::new(TARGET).join("etc/resolv.conf");
        if path.is_symlink() {
            io(fs::remove_file(&path))?;
        }
        self.write("/etc/resolv.conf", &content, 0o644)
    }
    fn prepare_external(&self) -> Result<(), Hint> {
        for tool in [
            "/usr/sbin/sgdisk",
            "/usr/sbin/partprobe",
            "/usr/bin/udevadm",
            "/usr/sbin/cryptsetup",
            "/usr/sbin/mkfs.btrfs",
            "/usr/sbin/mkfs.fat",
            "/usr/sbin/blkid",
            "/usr/bin/btrfs",
            "/usr/sbin/chroot",
        ] {
            if !Path::new(tool).is_file() {
                return Err(err(format!("The live environment is missing {tool}.")));
            }
        }
        if self.job.layout.boot.is_some()
            && self.job.layout.boot_fs == "ext4"
            && !Path::new("/usr/sbin/mkfs.ext4").is_file()
        {
            return Err(err("The live environment is missing mkfs.ext4."));
        }
        if matches!(self.job.plan.disk, borea_core::DiskPlan::Alongside { .. }) {
            for tool in ["/usr/sbin/ntfsresize", "/usr/sbin/parted"] {
                if !Path::new(tool).is_file() {
                    return Err(err(format!("The live environment is missing {tool}.")));
                }
            }
        }

        if !Path::new("/usr/bin/borea-firstboot").is_file() {
            return Err(err("The live environment is missing the target first-boot helper."));
        }
        io(fs::create_dir_all("/run/borea/external"))?;
        io(fs::set_permissions("/run/borea/external", fs::Permissions::from_mode(0o700)))?;
        for key in &self.job.external.repository_keys {
            if !borea_backend::external::identifier(&key.source) {
                return Err(err("Invalid repository key identifier."));
            }
            download(
                &key.url,
                &Path::new("/run/borea/external").join(format!("{}.asc", key.source)),
                Some(&key.sha256),
            )?;
        }
        for recipe in &self.job.external.aur {
            if !borea_backend::external::identifier(&recipe.base) {
                return Err(err("Invalid pinned AUR package base."));
            }
            download(
                &recipe.url,
                &Path::new("/run/borea/external").join(format!("{}.tar.gz", recipe.base)),
                Some(&recipe.sha256),
            )?;
        }
        for image in &self.job.external.appimages {
            if !borea_backend::external::identifier(&image.filename) {
                return Err(err("Invalid pinned AppImage filename."));
            }
            download(
                &image.url,
                &Path::new("/run/borea/external").join(&image.filename),
                Some(&image.sha256),
            )?;
        }
        for stage in &self.job.stages {
            for action in &stage.actions {
                match action {
                    Action::BuildAur { packages } => {
                        for pkg in packages {
                            if !self.job.external.aur.iter().any(|r| r.packages.contains(pkg)) {
                                return Err(err("An AUR package has not been pinned for Review."));
                            }
                        }
                    }
                    Action::InstallAppImages { releases } => {
                        for repo in releases {
                            if !self.job.external.appimages.iter().any(|r| &r.repository == repo) {
                                return Err(err(
                                    "An AppImage release has not been pinned for Review.",
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }
    fn aur(&mut self, packages: &[String], emit: &mut impl FnMut(Event)) -> Result<(), Hint> {
        if self.job.plan.distro != borea_core::Distribution::Arch {
            return Err(err("AUR packages can only be built for Arch."));
        }
        let _ = packages; // Dependency recipes were topologically resolved and pinned before Review.
        self.command(
            Root::Target,
            "/usr/bin/pacman",
            vec![
                "-S".into(),
                "--needed".into(),
                "--noconfirm".into(),
                "base-devel".into(),
                "git".into(),
            ],
            emit,
        )?;
        self.command(
            Root::Target,
            "/usr/bin/useradd",
            vec![
                "--system".into(),
                "--user-group".into(),
                "--create-home".into(),
                "--home-dir".into(),
                "/var/lib/borea-build".into(),
                "borea-build".into(),
            ],
            emit,
        )?;
        let recipes = self.job.external.aur.clone();
        for recipe in recipes {
            if !recipe.dependencies.is_empty() {
                self.command(
                    Root::Target,
                    "/usr/bin/pacman",
                    vec!["-S".into(), "--needed".into(), "--noconfirm".into()]
                        .into_iter()
                        .chain(recipe.dependencies)
                        .collect(),
                    emit,
                )?;
            }
            let destination = self.target_path(&format!("/var/lib/borea-build/{}", recipe.base))?;
            io(fs::create_dir_all(&destination))?;
            let copied = destination.join("recipe.tar.gz");
            io(fs::copy(format!("/run/borea/external/{}.tar.gz", recipe.base), &copied))?;
            self.command(
                Root::Target,
                "/usr/bin/chown",
                vec![
                    "-R".into(),
                    "borea-build:borea-build".into(),
                    format!("/var/lib/borea-build/{}", recipe.base),
                ],
                emit,
            )?;
            self.command(
                Root::Target,
                "/usr/sbin/runuser",
                vec![
                    "-u".into(),
                    "borea-build".into(),
                    "--".into(),
                    "/usr/bin/tar".into(),
                    "-xzf".into(),
                    format!("/var/lib/borea-build/{}/recipe.tar.gz", recipe.base),
                    "--strip-components=1".into(),
                    "-C".into(),
                    format!("/var/lib/borea-build/{}", recipe.base),
                    "--no-same-owner".into(),
                ],
                emit,
            )?;
            self.command(
                Root::Target,
                "/usr/sbin/runuser",
                vec![
                    "-u".into(),
                    "borea-build".into(),
                    "--".into(),
                    "/usr/bin/env".into(),
                    format!("--chdir=/var/lib/borea-build/{}", recipe.base),
                    "/usr/bin/makepkg".into(),
                    "--cleanbuild".into(),
                    "--noconfirm".into(),
                ],
                emit,
            )?;
            let mut artifacts = Vec::new();
            for entry in io(fs::read_dir(&destination))? {
                let entry = io(entry)?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".pkg.tar.zst") || name.ends_with(".pkg.tar.xz") {
                    if !io(entry.file_type())?.is_file() {
                        return Err(err("A built AUR artifact is not a regular file."));
                    }
                    let artifact = format!("/var/lib/borea-build/{}/{name}", recipe.base);
                    let metadata = self.command(
                        Root::Target,
                        "/usr/bin/pacman",
                        vec!["-Qp".into(), artifact.clone()],
                        emit,
                    )?;
                    if metadata
                        .split_whitespace()
                        .next()
                        .is_some_and(|name| recipe.packages.iter().any(|p| p == name))
                    {
                        artifacts.push(artifact);
                    }
                }
            }
            if artifacts.is_empty() {
                return Err(err("The AUR build did not produce an installable package."));
            }
            self.command(
                Root::Target,
                "/usr/bin/pacman",
                vec!["-U".into(), "--needed".into(), "--noconfirm".into()]
                    .into_iter()
                    .chain(artifacts)
                    .collect(),
                emit,
            )?;
        }
        self.command(
            Root::Target,
            "/usr/bin/userdel",
            vec!["--remove".into(), "borea-build".into()],
            emit,
        )?;
        Ok(())
    }
    fn appimages(&mut self, releases: &[String], emit: &mut impl FnMut(Event)) -> Result<(), Hint> {
        let images = self.job.external.appimages.clone();
        for image in images.iter().filter(|i| releases.contains(&i.repository)) {
            let id = image.repository.replace('/', "-");
            let dir = format!("/opt/borea/appimages/{id}");
            let destination = self.target_path(&dir)?;
            io(fs::create_dir_all(&destination))?;
            let binary = destination.join(&image.filename);
            io(fs::copy(Path::new("/run/borea/external").join(&image.filename), &binary))?;
            io(fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)))?;
            let user = self.job.plan.user.username.clone();
            self.command(
                Root::Target,
                "/usr/bin/chown",
                vec!["-R".into(), format!("{user}:{user}"), dir.clone()],
                emit,
            )?;
            self.command(
                Root::Target,
                "/usr/sbin/runuser",
                vec![
                    "-u".into(),
                    user,
                    "--".into(),
                    "/usr/bin/env".into(),
                    format!("--chdir={dir}"),
                    format!("{dir}/{}", image.filename),
                    "--appimage-extract".into(),
                ],
                emit,
            )?;
            let desktop = format!(
                "[Desktop Entry]\nType=Application\nName={}\nExec={dir}/squashfs-root/AppRun %U\nTerminal=false\nCategories=Utility;\n",
                image.repository.split('/').next_back().unwrap_or("Application")
            );
            self.write(&format!("/usr/local/share/applications/{id}.desktop"), &desktop, 0o644)?;
            self.write(
                &format!("/etc/borea/appimages/{id}.json"),
                &serde_json::to_string_pretty(&postinstall::ImageRecord {
                    image: image.clone(),
                    user: self.job.plan.user.username.clone(),
                })
                .map_err(|e| err(e.to_string()))?,
                0o644,
            )?;
        }
        if !images.is_empty() {
            self.command(
                Root::Target,
                "/usr/bin/systemctl",
                vec!["enable".into(), "borea-appimage-update.timer".into()],
                emit,
            )?;
        }
        Ok(())
    }
    fn install_helpers(&self) -> Result<(), Hint> {
        let destination = self.target_path("/usr/bin/borea-firstboot")?;
        io(fs::copy("/usr/bin/borea-firstboot", &destination))?;
        io(fs::set_permissions(destination, fs::Permissions::from_mode(0o755)))?;
        self.write("/var/lib/borea/firstboot.json", "[]", 0o600)?;
        self.write(
            "/etc/systemd/system/borea-firstboot.service",
            include_str!("../../../dist/borea-firstboot.service"),
            0o644,
        )?;
        self.write(
            "/etc/systemd/system/borea-appimage-update.service",
            include_str!("../../../dist/borea-appimage-update.service"),
            0o644,
        )?;
        self.write(
            "/etc/systemd/system/borea-appimage-update.timer",
            include_str!("../../../dist/borea-appimage-update.timer"),
            0o644,
        )?;
        Ok(())
    }
    fn copy_log(&self) -> Result<(), Hint> {
        let path = self.target_path("/var/log/borea/install.log")?;
        io(fs::create_dir_all(path.parent().ok_or_else(|| err("Invalid target log path."))?))?;
        io(fs::copy("/run/borea/install.log", &path))?;
        io(fs::set_permissions(path, fs::Permissions::from_mode(0o600)))
    }
    pub fn cleanup(&mut self) -> Result<(), Hint> {
        let mut errors = Vec::new();
        while let Some(path) = self.mounted.pop() {
            if let Err(e) = umount2(&path, MntFlags::empty()) {
                errors.push(format!("{}: {e}", path.display()));
            }
        }
        if self.opened_luks {
            match Command::new("/usr/sbin/cryptsetup")
                .args([
                    "close",
                    self.vars.get("MAPPER_NAME").map(String::as_str).unwrap_or("invalid-mapping"),
                ])
                .status()
            {
                Ok(s) if s.success() => {}
                Ok(_) => errors.push("Cannot close the encrypted root.".into()),
                Err(e) => errors.push(e.to_string()),
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(err(errors.join("; "))) }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.copy_log();
            let _ = self.cleanup();
        }
    }
}
pub fn download(url: &str, path: &Path, expected: Option<&str>) -> Result<(), Hint> {
    if !url.starts_with("https://") {
        return Err(err("Downloads require HTTPS."));
    }
    let agent = borea_http::agent(std::time::Duration::from_secs(1800))?;
    let mut response = agent.get(url).call().map_err(|_| err("The upstream download failed."))?;
    let mut reader = response.body_mut().as_reader();
    let mut f = io(OpenOptions::new().write(true).create_new(true).mode(0o600).open(path))?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = io(reader.read(&mut buffer))?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
        io(f.write_all(&buffer[..n]))?;
    }
    io(f.sync_all())?;
    if let Some(expected) = expected {
        if digest.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
            != expected.to_ascii_lowercase()
        {
            let _ = fs::remove_file(path);
            return Err(err("The upstream artifact SHA-256 did not match the reviewed plan."));
        }
    }
    Ok(())
}
pub mod postinstall;
