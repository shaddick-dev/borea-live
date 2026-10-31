//! The native Arch pathway. Disk/account choices stay local until an approved start.
use crate::handoff::{Disk, Service, Where};
use borea_core::{
    Desktop, DiskPlan, Identity, Keyboard, LocaleChoice, NetChoice, PartId, Plan, SourcePreference,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Storage,
    Account,
    Review,
    Running,
    Finished,
}
enum Answer {
    Disks,
    Review,
    Start,
    Events,
    Retry,
    Reboot,
}
struct Message {
    revision: u64,
    kind: Answer,
    result: Result<Value, String>,
}

pub struct NativeInstall {
    pub page: Page,
    derivative: Option<borea_core::ArchDerivative>,
    desktop: Desktop,
    pub disks: Vec<Disk>,
    pub pick: Option<Where>,
    pub user: Identity,
    pub locale: LocaleChoice,
    pub apps: BTreeSet<String>,
    /// Region fields the user has chosen; a suggestion from the connection never
    /// overwrites these.
    touched: BTreeSet<&'static str>,
    /// Encrypt the disk. Off by default; the Account page offers it only when the
    /// firmware is in Setup Mode.
    pub encrypt: bool,
    pub confirmed: bool,
    pub error: Option<String>,
    pub stage: String,
    pub progress: f32,
    password: Zeroizing<String>,
    confirmation: Zeroizing<String>,
    /// The separate sudo password, used when `user.same_passphrase` is off.
    admin: Zeroizing<String>,
    admin_confirmation: Zeroizing<String>,
    extras: BTreeSet<String>,
    secure_boot: bool,
    network: NetChoice,
    approval: Option<(Plan, String, Value)>,
    loading: bool,
    loaded: bool,
    revision: u64,
    job: Option<String>,
    cursor: u64,
    retry: Option<String>,
    stopped: bool,
    service: Service,
    tx: Sender<Message>,
    rx: Receiver<Message>,
}

impl NativeInstall {
    #[cfg(test)]
    pub(crate) fn derivative_for_test(&self) -> Option<borea_core::ArchDerivative> {
        self.derivative
    }
    pub(crate) fn composition(&mut self, desktop: Desktop) {
        if self.desktop != desktop && !self.busy() && self.page != Page::Finished {
            self.reset_storage();
            self.desktop = desktop;
        }
    }
    pub(crate) fn target(&mut self, cachyos: bool) {
        let derivative = cachyos.then_some(borea_core::ArchDerivative::Cachyos);
        if self.derivative != derivative && !self.busy() {
            self.reset_storage();
            self.derivative = derivative;
        }
    }
    pub(crate) fn reset_storage(&mut self) {
        if self.busy() || self.page == Page::Finished {
            return;
        }
        self.invalidate();
        self.pick = None;
        self.page = Page::Storage;
        self.forget_secrets();
    }
    fn forget_secrets(&mut self) {
        self.password.zeroize();
        self.confirmation.zeroize();
        self.admin.zeroize();
        self.admin_confirmation.zeroize();
    }
    /// Whether the account password is also the sudo password. Turning it back on
    /// forgets the separate one. The Account page's switch calls this.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn same_sudo(&mut self, same: bool) {
        if self.page != Page::Account || self.busy() {
            return;
        }
        self.invalidate();
        self.user.same_passphrase = same;
        if same {
            self.admin.zeroize();
            self.admin_confirmation.zeroize();
        }
    }
    /// The user chose this region field ("language", "keyboard", "console" or
    /// "timezone"); a suggestion from the connection leaves it alone from now on.
    pub(crate) fn touch(&mut self, kind: &'static str) {
        self.touched.insert(kind);
    }
    /// Fill the region from the connection's suggestion, sparing whatever the user
    /// already chose. Never once installing has begun.
    pub(crate) fn suggest_region(&mut self, guess: LocaleChoice) {
        if self.busy() || matches!(self.page, Page::Review | Page::Running | Page::Finished) {
            return;
        }
        let l = &mut self.locale;
        if !self.touched.contains("language") {
            l.lang = guess.lang;
        }
        if !self.touched.contains("timezone") {
            l.timezone = guess.timezone;
        }
        if !self.touched.contains("keyboard") {
            l.keyboard.layout = guess.keyboard.layout;
            l.keyboard.variant = guess.keyboard.variant;
        }
        if !self.touched.contains("console") {
            l.keyboard.console = guess.keyboard.console;
        }
    }
    pub(crate) fn edit_account(&mut self, field: &str, value: String) {
        if self.page != Page::Account || self.busy() {
            return;
        }
        self.invalidate();
        match field {
            "full-name" => self.user.full_name = value,
            "username" => self.user.username = value,
            "hostname" => self.user.hostname = value,
            "password" => *self.password = value,
            "confirmation" => *self.confirmation = value,
            "admin" => *self.admin = value,
            "admin-confirmation" => *self.admin_confirmation = value,
            _ => {}
        }
    }
    pub(crate) fn review_details(&self) -> Option<String> {
        let (plan, _, review) = self.approval.as_ref()?;
        let storage = match &plan.disk {
            DiskPlan::Whole { .. } => {
                "Erase this entire drive. All existing data on it will be lost.".to_owned()
            }
            DiskPlan::Alongside { windows_part, linux_size, .. } => format!(
                "Shrink {} and use {} GiB for Linux. Keep Windows and its EFI files.",
                windows_part.path.display(),
                linux_size / crate::handoff::GIB
            ),
        };
        let packages = review["review"]["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| {
                let name = p["name"].as_str().or_else(|| p.as_str())?;
                Some(match p["source"].as_str() {
                    Some("aur") => format!("{name} (AUR)"),
                    Some("flathub") => format!("{name} (Flatpak)"),
                    _ => name.to_owned(),
                })
            })
            .collect::<Vec<_>>()
            .join(", ");
        let warnings = review["review"]["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        Some(format!(
            "Drive: {}\n{storage}\n{}\n\nAccount: {} · {}\nLanguage: {}\nKeyboard: {} ({}){}\nTime zone: {}\n\nRequired packages: {packages}\n\n{warnings}",
            plan.disk.disk().path.display(),
            if plan.encrypt {
                "Encrypted Linux storage · LUKS2 and Btrfs"
            } else {
                "Linux storage · Btrfs"
            },
            plan.user.username,
            plan.user.hostname,
            plan.locale.lang,
            plan.locale.keyboard.layout,
            plan.locale.keyboard.variant,
            if plan.encrypt {
                format!("\nDisk unlock keyboard: {}", plan.locale.keyboard.console)
            } else {
                String::new()
            },
            plan.locale.timezone
        ))
    }
    pub(crate) fn loading(&self) -> bool {
        self.loading
    }
    pub(crate) fn refresh_disks(&mut self) {
        self.loaded = false;
        self.load();
    }
    pub(crate) fn retry_progress(&mut self) {
        if let Some(action) = self.retry.clone() {
            self.ask("install.retry", json!({"action_id":action}), Answer::Retry);
        } else if !self.stopped {
            self.error = None;
            self.follow();
        }
    }
    pub(crate) fn reboot(&self) {
        if self.page == Page::Finished || self.stopped {
            self.ask("system.reboot", json!({"firmware_setup":false}), Answer::Reboot);
        }
    }
    pub fn with_service(service: Service) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            page: Page::Storage,
            derivative: None,
            desktop: Desktop::HyprlandNoctalia,
            disks: vec![],
            pick: None,
            user: Identity {
                full_name: String::new(),
                username: String::new(),
                hostname: String::new(),
                same_passphrase: true,
            },
            locale: LocaleChoice {
                lang: String::new(),
                timezone: String::new(),
                keyboard: Keyboard {
                    layout: String::new(),
                    variant: String::new(),
                    console: String::new(),
                },
            },
            apps: BTreeSet::new(),
            touched: BTreeSet::new(),
            encrypt: false,
            confirmed: false,
            error: None,
            stage: String::new(),
            progress: 0.0,
            password: Zeroizing::new(String::new()),
            confirmation: Zeroizing::new(String::new()),
            admin: Zeroizing::new(String::new()),
            admin_confirmation: Zeroizing::new(String::new()),
            extras: BTreeSet::new(),
            secure_boot: false,
            network: NetChoice::Ethernet,
            approval: None,
            loading: false,
            loaded: false,
            revision: 0,
            job: None,
            cursor: 0,
            retry: None,
            stopped: false,
            service,
            tx,
            rx,
        }
    }
    pub fn system() -> Self {
        Self::with_service(Arc::new(|method, params| {
            borea_protocol::call(method, params, Duration::from_secs(600))
                .map_err(|e| e.to_string())
        }))
    }
    pub(crate) fn stopped(&self) -> bool {
        self.stopped
    }
    pub fn busy(&self) -> bool {
        self.page == Page::Running
    }
    pub fn invalidate(&mut self) {
        if self.busy() || self.page == Page::Finished {
            return;
        }
        self.revision += 1;
        if self.page == Page::Review {
            self.page = Page::Account;
        }
        self.approval = None;
        self.confirmed = false;
        self.loading = false;
        self.error = None;
    }
    pub fn choices(&mut self, extras: BTreeSet<String>, secure_boot: bool, network: NetChoice) {
        if self.extras != extras || self.secure_boot != secure_boot || self.network != network {
            self.invalidate();
            self.extras = extras;
            self.secure_boot = secure_boot;
            self.network = network;
        }
    }
    fn ask(&self, method: &'static str, mut params: Value, kind: Answer) {
        let (service, tx, revision) = (Arc::clone(&self.service), self.tx.clone(), self.revision);
        if thread::Builder::new()
            .name("native-install".into())
            .spawn(move || {
                let result = service(method, &params);
                if let Some(Value::String(secret)) = params.pointer_mut("/secrets/passphrase") {
                    secret.zeroize();
                }
                let _ = tx.send(Message { revision, kind, result });
            })
            .is_err()
        {
            let _ = self.tx.send(Message {
                revision: self.revision,
                kind: Answer::Reboot,
                result: Err("Cannot start the installer request thread.".into()),
            });
        }
    }
    pub fn load(&mut self) {
        if !self.loaded && !self.loading {
            self.loading = true;
            self.ask("disks.list", Value::Null, Answer::Disks);
        }
    }
    pub fn plan(&self) -> Result<Plan, String> {
        let disk = match &self.pick {
            Some(Where::Whole { disk }) => DiskPlan::Whole {
                disk: serde_json::from_value(
                    self.disks.get(*disk).ok_or("Choose a disk.")?.id.clone(),
                )
                .map_err(|_| "Invalid disk identity.")?,
            },
            Some(Where::Alongside { disk, part, size }) => {
                let d = self.disks.get(*disk).ok_or("Choose a disk.")?;
                let p = d.parts.get(*part).ok_or("Choose the Windows partition.")?;
                DiskPlan::Alongside {
                    disk: serde_json::from_value(d.id.clone())
                        .map_err(|_| "Invalid disk identity.")?,
                    windows_part: PartId {
                        path: p.path.clone().into(),
                        uuid: p.uuid.clone(),
                        number: p.number,
                    },
                    linux_size: *size,
                }
            }
            None => return Err("Choose where Arch will be installed.".into()),
        };
        let plan = Plan {
            version: borea_core::PLAN_VERSION,
            encrypt: self.encrypt,
            distro: borea_core::Distribution::Arch,
            derivative: self.derivative,
            release: "rolling".into(),
            desktop: Some(self.desktop),
            env: "composed".into(),
            secure_boot: Some(self.secure_boot),
            boot_arguments: vec![],
            net: self.network.clone(),
            locale: self.locale.clone(),
            user: self.user.clone(),
            disk,
            extras: self.extras.clone(),
            apps: self.apps.clone(),
            addons: BTreeSet::new(),
            picks: vec![],
            prefer: SourcePreference::Aur,
            sources: Default::default(),
        };
        plan.validate_intent()
            .and_then(|_| borea_locale::validate(&plan.locale))
            .map_err(|e| e.to_string())?;
        Ok(plan)
    }
    fn credentials_valid(&self) -> Result<(), String> {
        if *self.password != *self.confirmation {
            return Err("The passwords do not match.".into());
        }
        let separate = !self.user.same_passphrase;
        if separate && *self.admin != *self.admin_confirmation {
            return Err("The sudo passwords do not match.".into());
        }
        borea_core::Secrets {
            wifi: None,
            passphrase: borea_core::Secret::new(self.password.to_string()),
            admin: separate.then(|| borea_core::Secret::new(self.admin.to_string())),
        }
        .validate(&self.user)
        .map_err(|e| e.to_string())
    }
    pub fn review(&mut self) {
        let result = self.plan().and_then(|plan| self.credentials_valid().map(|_| plan));
        match result {
            Ok(plan) => {
                self.invalidate();
                self.loading = true;
                self.ask("plan.review", json!({"plan":plan}), Answer::Review);
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub fn ready(&self) -> bool {
        self.page == Page::Review && self.approval.is_some() && self.confirmed && !self.loading
    }
    pub fn start(&mut self) {
        if !self.ready() {
            return;
        }
        if let Err(e) = self.credentials_valid() {
            self.error = Some(e);
            return;
        }
        let Some((plan, approval, _)) = &self.approval else {
            return;
        };
        let admin = (!self.user.same_passphrase).then(|| self.admin.as_str());
        let params = json!({"plan":plan,"approval":approval,"secrets":{"wifi":null,"passphrase":self.password.as_str(),"admin":admin}});
        self.page = Page::Running;
        self.stage = "Checking sources before writing…".into();
        self.error = None;
        self.ask("install.start", params, Answer::Start);
        self.forget_secrets();
    }
    fn follow(&self) {
        let (service, tx, revision, mut cursor) =
            (Arc::clone(&self.service), self.tx.clone(), self.revision, self.cursor);
        let _ = thread::Builder::new().name("native-events".into()).spawn(move || {
            loop {
                let result = service("install.events", &json!({"after":cursor}));
                let stop = result.as_ref().map_or(true, |b| {
                    b["events"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|e| matches!(e["event"].as_str(), Some("finished" | "failed")))
                });
                if let Ok(batch) = &result {
                    cursor = batch["cursor"].as_u64().unwrap_or(cursor);
                }
                let _ = tx.send(Message { revision, kind: Answer::Events, result });
                if stop {
                    break;
                }
                thread::sleep(Duration::from_millis(400));
            }
        });
    }
    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            if msg.revision != self.revision {
                continue;
            }
            match msg.result {
                Err(e) => {
                    self.error = Some(e);
                    self.loading = false;
                    if matches!(msg.kind, Answer::Start) {
                        self.page = Page::Account;
                        self.approval = None;
                    }
                }
                Ok(value) => match msg.kind {
                    Answer::Disks => {
                        self.loading = false;
                        match serde_json::from_value(value["disks"].clone()) {
                            Ok(disks) => {
                                self.disks = disks;
                                self.loaded = true;
                            }
                            Err(_) => {
                                self.error =
                                    Some("The service returned an invalid disk list.".into())
                            }
                        }
                    }
                    Answer::Review => {
                        self.loading = false;
                        match (self.plan(), value["approval"].as_str()) {
                            (Ok(plan), Some(approval)) => {
                                self.approval = Some((plan, approval.into(), value));
                                self.page = Page::Review;
                            }
                            _ => self.error = Some("The installation review is incomplete.".into()),
                        }
                    }
                    Answer::Start => {
                        self.job = value["job_id"].as_str().map(str::to_owned);
                        self.cursor = 0;
                        self.follow();
                    }
                    Answer::Retry => {
                        self.error = None;
                        self.retry = None;
                        self.follow();
                    }
                    Answer::Reboot => {}
                    Answer::Events => {
                        if value["job_id"].as_str() != self.job.as_deref() {
                            self.error =
                                Some("The service is reporting a different installation.".into());
                            continue;
                        }
                        self.cursor = value["cursor"].as_u64().unwrap_or(self.cursor);
                        for event in value["events"].as_array().into_iter().flatten() {
                            match event["event"].as_str() {
                                Some("stage-started") => {
                                    self.stage = event["title"].as_str().unwrap_or_default().into()
                                }
                                Some("progress") => {
                                    self.progress = event["fraction"].as_f64().unwrap_or(0.0) as f32
                                }
                                Some("failed") => {
                                    self.stopped = event["fault"]["retryable"] != true;
                                    self.error = Some(
                                        event["fault"]["title"]
                                            .as_str()
                                            .unwrap_or("Installation failed.")
                                            .into(),
                                    );
                                    if event["fault"]["retryable"] == true {
                                        self.retry =
                                            event["fault"]["action_id"].as_str().map(str::to_owned);
                                    }
                                }
                                Some("finished") => {
                                    self.page = Page::Finished;
                                    self.progress = 1.0;
                                }
                                _ => {}
                            }
                        }
                    }
                },
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    fn wait(link: &mut NativeInstall, done: impl Fn(&NativeInstall) -> bool) {
        for _ in 0..300 {
            link.poll();
            if done(link) {
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        panic!("installer response did not arrive");
    }
    fn account(link: &mut NativeInstall) {
        link.user = Identity {
            full_name: "Test Person".into(),
            username: "tester".into(),
            hostname: "test-pc".into(),
            same_passphrase: true,
        };
        link.locale = LocaleChoice {
            lang: "en_GB.UTF-8".into(),
            timezone: "Europe/London".into(),
            keyboard: Keyboard {
                layout: "gb".into(),
                variant: String::new(),
                console: "uk".into(),
            },
        };
        *link.password = "disposable-test-only".into();
        *link.confirmation = link.password.to_string();
    }
    fn service(calls: Arc<Mutex<Vec<String>>>) -> Service {
        Arc::new(move |method, params| {
            calls.lock().unwrap().push(method.into());
            Ok(match method {
                "disks.list" => {
                    json!({"disks":[{"id":{"path":"/dev/vda","model":"Test disk","serial":"TEST","size":107374182400_u64,"logical_sector":512},"parts":[{"path":"/dev/vda3","number":3,"uuid":"WINDOWS","fs":"ntfs","max_linux_size":50*crate::handoff::GIB}]}]})
                }
                "plan.review" => {
                    assert!(params.get("secrets").is_none());
                    json!({"approval":"reviewed-digest","external":{"noctalia":"5.4.1"},"review":{"packages":[],"warnings":[]},"layout":{}})
                }
                "install.start" => {
                    assert_eq!(params["approval"], "reviewed-digest");
                    assert_eq!(params["secrets"]["passphrase"], "disposable-test-only");
                    json!({"job_id":"job-1"})
                }
                "install.events" => {
                    json!({"job_id":"job-1","cursor":2,"events":[{"event":"stage-started","title":"Installing Arch"},{"event":"finished"}]})
                }
                _ => Value::Null,
            })
        })
    }
    #[test]
    fn a_region_from_the_connection_spares_what_the_user_chose() {
        let mut link = NativeInstall::with_service(service(Arc::default()));
        link.page = Page::Account;
        link.locale.timezone = "Europe/Berlin".into();
        link.touch("timezone");
        link.suggest_region(borea_locale::guess("GB", Some("Europe/London")).unwrap());
        assert_eq!(link.locale.lang, "en_GB.UTF-8");
        assert_eq!(link.locale.keyboard.layout, "gb");
        assert_eq!(link.locale.keyboard.console, "uk");
        assert_eq!(link.locale.timezone, "Europe/Berlin", "the user's choice stands");
        link.page = Page::Review;
        link.suggest_region(borea_locale::guess("US", None).unwrap());
        assert_eq!(link.locale.lang, "en_GB.UTF-8", "never once reviewing");
    }
    #[test]
    fn any_length_password_and_a_separate_sudo_password_are_accepted() {
        let mut link = NativeInstall::with_service(service(Arc::default()));
        link.page = Page::Account;
        account(&mut link);
        link.edit_account("password", "a".into());
        link.edit_account("confirmation", "a".into());
        assert!(link.credentials_valid().is_ok(), "no minimum length");
        link.same_sudo(false);
        assert!(!link.user.same_passphrase);
        link.edit_account("admin", "sudo-only".into());
        assert_eq!(link.credentials_valid().unwrap_err(), "The sudo passwords do not match.");
        link.edit_account("admin-confirmation", "sudo-only".into());
        assert!(link.credentials_valid().is_ok());
        link.same_sudo(true);
        assert!(
            link.admin.is_empty() && link.admin_confirmation.is_empty(),
            "forgotten when turned back on"
        );
    }
    #[test]
    fn whole_and_alongside_require_review_and_confirmation_before_start() {
        for alongside in [false, true] {
            let calls = Arc::new(Mutex::new(vec![]));
            let locale = LocaleChoice {
                lang: "fr_FR.UTF-8".into(),
                timezone: "Asia/Tokyo".into(),
                keyboard: Keyboard {
                    layout: "de".into(),
                    variant: "nodeadkeys".into(),
                    console: "de-latin1".into(),
                },
            };
            let expected_locale = serde_json::to_value(&locale).unwrap();
            let mock = service(calls.clone());
            let mut link = NativeInstall::with_service(Arc::new(move |method, params| {
                if matches!(method, "plan.review" | "install.start") {
                    assert_eq!(params["plan"]["locale"], expected_locale);
                }
                mock(method, params)
            }));
            link.load();
            wait(&mut link, |l| l.loaded);
            assert!(link.pick.is_none());
            assert!(link.apps.is_empty());
            assert!(link.extras.is_empty());
            link.start();
            assert!(!calls.lock().unwrap().iter().any(|s| s == "install.start"));
            account(&mut link);
            link.locale = locale;
            link.pick = Some(if alongside {
                Where::Alongside { disk: 0, part: 0, size: 45 * crate::handoff::GIB }
            } else {
                Where::Whole { disk: 0 }
            });
            let plan = link.plan().unwrap();
            assert_eq!(matches!(plan.disk, DiskPlan::Alongside { .. }), alongside);
            link.review();
            wait(&mut link, |l| l.page == Page::Review);
            assert!(!link.ready());
            link.start();
            assert!(!calls.lock().unwrap().iter().any(|s| s == "install.start"));
            link.confirmed = true;
            link.start();
            assert!(link.password.is_empty());
            wait(&mut link, |l| l.page == Page::Finished);
            assert_eq!(
                calls.lock().unwrap().iter().filter(|s| s.as_str() == "install.start").count(),
                1
            );
        }
    }
    #[test]
    fn changed_choices_discard_a_pending_review() {
        let mut link = NativeInstall::with_service(Arc::new(|_, _| Ok(Value::Null)));
        let old_revision = link.revision;
        link.choices(["gaming".into()].into(), false, NetChoice::Ethernet);
        link.tx
            .send(Message {
                revision: old_revision,
                kind: Answer::Review,
                result: Ok(json!({"approval":"stale"})),
            })
            .unwrap();
        link.poll();
        assert!(link.approval.is_none());
        assert!(!link.ready());
    }
}
