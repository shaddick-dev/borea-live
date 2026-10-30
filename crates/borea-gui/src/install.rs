//! The native Arch pathway. Disk/account choices stay local until an approved start.
use crate::handoff::{Disk, Service, Where};
use borea_core::{
    Desktop, DiskPlan, Identity, Keyboard, LocaleChoice, NetChoice, PartId, Plan, SourcePreference,
};
use eframe::egui::{self, Ui};
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
    pub disks: Vec<Disk>,
    pub pick: Option<Where>,
    pub user: Identity,
    pub locale: LocaleChoice,
    pub apps: BTreeSet<String>,
    pub confirmed: bool,
    pub error: Option<String>,
    pub stage: String,
    pub progress: f32,
    password: Zeroizing<String>,
    confirmation: Zeroizing<String>,
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
    ctx: egui::Context,
}

impl NativeInstall {
    pub fn context(&self) -> egui::Context {
        self.ctx.clone()
    }
    pub fn with_service(ctx: egui::Context, service: Service) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            page: Page::Storage,
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
            confirmed: false,
            error: None,
            stage: String::new(),
            progress: 0.0,
            password: Zeroizing::new(String::new()),
            confirmation: Zeroizing::new(String::new()),
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
            ctx,
        }
    }
    pub fn system(ctx: egui::Context) -> Self {
        Self::with_service(
            ctx,
            Arc::new(|method, params| {
                borea_protocol::call(method, params, Duration::from_secs(600))
                    .map_err(|e| e.to_string())
            }),
        )
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
        let (service, tx, ctx, revision) =
            (Arc::clone(&self.service), self.tx.clone(), self.ctx.clone(), self.revision);
        if thread::Builder::new()
            .name("native-install".into())
            .spawn(move || {
                let result = service(method, &params);
                if let Some(Value::String(secret)) = params.pointer_mut("/secrets/passphrase") {
                    secret.zeroize();
                }
                let _ = tx.send(Message { revision, kind, result });
                ctx.request_repaint();
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
            version: 1,
            distro: borea_core::Distribution::Arch,
            release: "rolling".into(),
            desktop: Some(Desktop::HyprlandNoctalia),
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
            prefer: SourcePreference::Native,
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
        borea_core::Secrets {
            wifi: None,
            passphrase: borea_core::Secret::new(self.password.to_string()),
            admin: None,
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
        let params = json!({"plan":plan,"approval":approval,"secrets":{"wifi":null,"passphrase":self.password.as_str(),"admin":null}});
        self.page = Page::Running;
        self.stage = "Checking sources before writing…".into();
        self.error = None;
        self.ask("install.start", params, Answer::Start);
        self.password.zeroize();
        self.confirmation.zeroize();
    }
    fn follow(&self) {
        let (service, tx, ctx, revision, mut cursor) = (
            Arc::clone(&self.service),
            self.tx.clone(),
            self.ctx.clone(),
            self.revision,
            self.cursor,
        );
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
                ctx.request_repaint();
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
    pub fn ui(&mut self, ui: &mut Ui) {
        ui.set_max_width(660.0);
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(208, 135, 112), error);
            ui.add_space(8.0);
        }
        match self.page {
            Page::Storage => {
                ui.heading("Where should Arch go?");
                ui.label("Choose a drive and layout. Nothing is selected automatically.");
                let mut selected = self.pick.clone();
                for (i, disk) in self
                    .disks
                    .iter()
                    .enumerate()
                    .filter(|(_, d)| d.usable() && d.size() >= 40 * crate::handoff::GIB)
                {
                    ui.separator();
                    ui.label(format!("{} · {}", disk.name(), disk.path()));
                    if ui
                        .radio(
                            matches!(selected, Some(Where::Whole {disk}) if disk == i),
                            "Erase this disk",
                        )
                        .clicked()
                    {
                        selected = Some(Where::Whole { disk: i });
                    }
                    for (j, p) in disk.parts.iter().enumerate() {
                        let Some(max) =
                            p.max_linux_size.filter(|max| *max >= 40 * crate::handoff::GIB)
                        else {
                            continue;
                        };
                        if ui.radio(matches!(selected, Some(Where::Alongside {disk,part,..}) if disk == i && part == j), format!("Alongside Windows on {}", p.path)).clicked() {
                            selected = Some(Where::Alongside {disk:i,part:j,size: (60 * crate::handoff::GIB).min(max)});
                        }
                        if let Some(Where::Alongside { disk, part, size }) = &mut selected
                            && *disk == i
                            && *part == j
                        {
                            let mut gib = *size / crate::handoff::GIB;
                            ui.add(
                                egui::Slider::new(&mut gib, 40..=max / crate::handoff::GIB)
                                    .text("Linux space (GiB)"),
                            );
                            *size = gib * crate::handoff::GIB;
                        }
                    }
                }
                if selected != self.pick {
                    self.invalidate();
                    self.pick = selected;
                }
                if self.loading {
                    ui.spinner();
                }
                if self.disks.is_empty() && !self.loading && ui.button("Refresh disks").clicked() {
                    self.loaded = false;
                    self.load();
                }
                ui.add_space(12.0);
                if ui
                    .add_enabled(self.pick.is_some(), egui::Button::new("Account and region"))
                    .clicked()
                {
                    self.page = Page::Account;
                }
            }
            Page::Account => {
                ui.heading("Your account");
                let mut changed = false;
                for (label, value) in [
                    ("Full name", &mut self.user.full_name),
                    ("Username", &mut self.user.username),
                    ("Computer name", &mut self.user.hostname),
                ] {
                    let name = ui.label(label);
                    changed |= ui.text_edit_singleline(value).labelled_by(name.id).changed();
                }
                let name = ui.label("Password");
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut *self.password).password(true))
                    .labelled_by(name.id)
                    .changed();
                let name = ui.label("Confirm password");
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut *self.confirmation).password(true))
                    .labelled_by(name.id)
                    .changed();
                ui.label("This password unlocks your encrypted Linux disk and signs you in.");
                ui.add_space(10.0);
                ui.heading("Region");
                changed |= choose(
                    ui,
                    "Language",
                    &mut self.locale.lang,
                    borea_locale::LOCALES.iter().map(|v| (v.id.to_owned(), v.english.to_owned())),
                );
                let mut keyboard =
                    format!("{}:{}", self.locale.keyboard.layout, self.locale.keyboard.variant);
                if choose(
                    ui,
                    "Keyboard",
                    &mut keyboard,
                    borea_locale::KEYBOARDS
                        .iter()
                        .map(|v| (format!("{}:{}", v.layout, v.variant), v.description.to_owned())),
                ) {
                    if let Some((layout, variant)) = keyboard.split_once(':') {
                        self.locale.keyboard.layout = layout.into();
                        self.locale.keyboard.variant = variant.into();
                    }
                    // Console keys are chosen separately because XKB variants are not 1:1.
                    changed = true;
                }
                changed |= choose(
                    ui,
                    "Keyboard for disk unlock",
                    &mut self.locale.keyboard.console,
                    borea_locale::console_keymaps().map(|key| (key.to_owned(), key.to_owned())),
                );
                changed |= choose(
                    ui,
                    "Time zone",
                    &mut self.locale.timezone,
                    borea_locale::TIMEZONES.iter().map(|v| (v.to_string(), v.replace('_', " "))),
                );
                ui.add_space(10.0);
                ui.heading("Extra apps");
                ui.label("These finish quietly after your first login.");
                egui::CollapsingHeader::new("Choose applications").show(ui, |ui| {
                    let sources = borea_catalog::Sources::defaults(borea_catalog::Distro::Arch);
                    for app in borea_catalog::APPS {
                        let available = matches!(
                            borea_catalog::resolve(app, &sources, borea_catalog::Prefer::Native),
                            borea_catalog::Resolution::From {
                                source: borea_catalog::Source::Repo(_)
                                    | borea_catalog::Source::Flathub,
                                ..
                            }
                        );
                        let mut on = self.apps.contains(app.id);
                        if ui
                            .add_enabled(available, egui::Checkbox::new(&mut on, app.name))
                            .changed()
                        {
                            if on {
                                self.apps.insert(app.id.into());
                            } else {
                                self.apps.remove(app.id);
                            }
                            changed = true;
                        }
                    }
                });
                if changed {
                    self.invalidate();
                }
                if self.loading {
                    ui.spinner();
                } else {
                    if ui.button("Check installation plan").clicked() {
                        self.review();
                    }
                    if ui.button("Back to storage").clicked() {
                        self.invalidate();
                        self.page = Page::Storage;
                    }
                }
            }
            Page::Review => {
                ui.heading("Ready to install Arch");
                if let Some((plan, _, review)) = &self.approval {
                    ui.label(format!("Drive: {}", plan.disk.disk().path.display()));
                    ui.label(match plan.disk {
                        DiskPlan::Whole { .. } => "Erase the selected disk",
                        _ => "Shrink Windows and install alongside",
                    });
                    ui.label(format!("Account: {} · {}", plan.user.username, plan.user.hostname));
                    ui.label(format!(
                        "{} · {} · {}",
                        plan.locale.lang, plan.locale.keyboard.layout, plan.locale.timezone
                    ));
                    ui.label("Encrypted Linux storage · Hyprland + Noctalia · Noctalia Greeter");
                    ui.label(format!(
                        "Noctalia {}",
                        review["external"]["noctalia"].as_str().unwrap_or("v5")
                    ));
                    for warning in review["review"]["warnings"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                    {
                        ui.label(warning);
                    }
                    egui::CollapsingHeader::new("Packages and disk changes").show(ui,|ui| { ui.monospace(serde_json::to_string_pretty(&json!({"packages":review["review"]["packages"],"storage":review["layout"]})).unwrap_or_default()); });
                }
                ui.checkbox(
                    &mut self.confirmed,
                    "I confirm these disk changes and have backed up important files.",
                );
                if ui.add_enabled(self.ready(), egui::Button::new("Install Arch")).clicked() {
                    self.start();
                }
                if ui.button("Edit choices").clicked() {
                    self.invalidate();
                    self.page = Page::Account;
                }
            }
            Page::Running => {
                ui.heading(&self.stage);
                ui.add(egui::ProgressBar::new(self.progress).show_percentage());
                ui.label("Keep the Borea USB connected until installation finishes.");
                if let Some(action) = self.retry.clone()
                    && ui.button("Retry this step").clicked()
                {
                    self.ask("install.retry", json!({"action_id":action}), Answer::Retry);
                }
                if self.error.is_some()
                    && self.retry.is_none()
                    && !self.stopped
                    && ui.button("Reconnect progress").clicked()
                {
                    self.error = None;
                    self.follow();
                }
                if self.stopped {
                    ui.label("Installation stopped. Check the error before restarting from the Borea USB.");
                    if ui.button("Restart this computer").clicked() {
                        self.ask("system.reboot", json!({"firmware_setup":false}), Answer::Reboot);
                    }
                }
            }
            Page::Finished => {
                ui.heading("Arch is installed");
                ui.label("Restart, unlock the disk, then sign in through Noctalia Greeter. Your extra apps will install in the background.");
                if ui.button("Restart into Arch").clicked() {
                    self.ask("system.reboot", json!({"firmware_setup":false}), Answer::Reboot);
                }
            }
        }
    }
}

fn choose(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    items: impl Iterator<Item = (String, String)>,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label(label)
        .selected_text(if value.is_empty() || value == ":" { "Choose…" } else { value.as_str() })
        .height(200.0)
        .show_ui(ui, |ui| {
            for (id, name) in items {
                changed |= ui.selectable_value(value, id, name).changed();
            }
        });
    changed
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
    fn whole_and_alongside_require_review_and_confirmation_before_start() {
        for alongside in [false, true] {
            let calls = Arc::new(Mutex::new(vec![]));
            let mut link =
                NativeInstall::with_service(egui::Context::default(), service(calls.clone()));
            link.load();
            wait(&mut link, |l| l.loaded);
            assert!(link.pick.is_none());
            assert!(link.apps.is_empty());
            assert!(link.extras.is_empty());
            link.start();
            assert!(!calls.lock().unwrap().iter().any(|s| s == "install.start"));
            account(&mut link);
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
        let mut link =
            NativeInstall::with_service(egui::Context::default(), Arc::new(|_, _| Ok(Value::Null)));
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
