//! Render-independent installer workflow. Workers communicate through channels.
use crate::firmware::FirmwareLink;
use crate::handoff::{self, HandoffLink, Phase, Where};
use crate::model::{DesktopMode, Selection, ShellPick};
use crate::wifi::{WifiForm, WifiLink};
use borea_distro::adapter;
use borea_distro::secure_boot::{self, Readiness};
use borea_hardware::HardwareSnapshot;
use borea_live_catalog::{Catalog, Distro, Edition};
use borea_live_core::{Fact, Family, FirmwareMode};
use std::sync::mpsc::{self, Receiver};
use std::thread;
const HANDOFF_TARGET: &str = "omarchy";
#[path = "desktop.rs"]
pub mod desktop;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    Connect,
    Machine,
    System,
    Distribution,
    Desktop,
    Review,
}

impl Step {
    pub const ALL: [Step; 6] = [
        Step::Connect,
        Step::Machine,
        Step::System,
        Step::Distribution,
        Step::Desktop,
        Step::Review,
    ];

    fn number(self) -> usize {
        Step::ALL.iter().position(|s| *s == self).unwrap_or(0) + 1
    }

    fn name(self) -> &'static str {
        match self {
            Step::Connect => "Connect",
            Step::Machine => "Machine",
            Step::System => "Family",
            Step::Distribution => "Distribution",
            Step::Desktop => "Desktop",
            Step::Review => "Review",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Step::Connect => "Catch a connection.",
            Step::Machine => "A sense of your system.",
            Step::System => "Choose a distribution family.",
            Step::Distribution => "Choose your distribution.",
            Step::Desktop => "Compose your desktop.",
            Step::Review => "Ready when you are.",
        }
    }

    fn lead(self) -> &'static str {
        match self {
            Step::Connect => "Borea downloads your system as it installs. Connect once, here.",
            Step::Machine => "Read from this machine, read-only. Unknown stays unknown.",
            Step::System => "Choose a family to see its distributions on the next page.",
            Step::Distribution => {
                "From the project's upstream sources, with its own releases and updates."
            }
            Step::Desktop => {
                "Editions without a desktop let you choose one. Every step can stop here."
            }
            Step::Review => "Check everything once. Nothing has been written to any disk.",
        }
    }

    fn next(self) -> Option<Step> {
        Step::ALL.get(self.number()).copied()
    }

    fn back(self) -> Option<Step> {
        self.number().checked_sub(2).and_then(|i| Step::ALL.get(i)).copied()
    }
}

/// Everything a user can do, recorded during rendering and applied once per frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Go(Step),
    Next,
    Back,
    Family(Family),
    Distro(Family, usize),
    Edition(Family, usize),
    Variant(Family, usize),
    Mode(Family, DesktopMode),
    Compositor(Family, String),
    Shell(Family, ShellPick),
    Extra(Family, String, bool),
    Compose(Family, (usize, usize)),
    SecureBoot(bool),
    ToggleTheme,
    ToggleMotion,
    OpenFirmware,
    CancelFirmware,
    RestartFirmware,
    /// Where a hand-off install goes (Omarchy): whole disk or beside Windows.
    HandoffChoose(Where),
    HandoffConfirm(bool),
    HandoffReview,
    HandoffRetry,
}

enum Scan {
    Pending(Receiver<HardwareSnapshot>),
    Done(Box<HardwareSnapshot>),
}

pub struct LiveInstaller {
    catalog: Catalog,
    selection: Selection,
    dark: bool,
    still: bool,
    /// The wind's pace: gusts on page changes, rising while installing.

    /// The step the wind last saw, to notice page changes.
    scan: Scan,
    wifi: WifiLink,
    form: WifiForm,
    step: Step,
    /// The furthest step reached; the rail lets you return to any step up to it.
    reached: Step,
    // The legacy card model stores indexed choices. These explicit decisions gate
    // the live wizard so its internal placeholders can never become user choices.
    picked_family: Option<Family>,
    picked_distro: bool,
    picked_edition: bool,
    picked_variant: bool,
    picked_mode: bool,
    picked_compositor: bool,
    picked_shell: bool,
    firmware: FirmwareLink,
    firmware_dialog: bool,
    /// Omarchy installs through its own installer, started from this stick.
    handoff: HandoffLink,
    native: crate::install::NativeInstall,
    region: RegionLookup,
}

/// The region from the connection: asked once, when the real installer first comes
/// online. Previews and captures never ask.
enum RegionLookup {
    Off,
    Waiting,
    Asking(Receiver<Option<borea_net::GeoSuggestion>>),
    Done,
}

impl LiveInstaller {
    pub const CAN_INSTALL: bool = false;
    fn for_desktop() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("live-scan".into())
            .spawn(move || {
                let _ = tx.send(borea_hardware::scan());
            })
            .expect("spawn hardware scan");
        let dark = true;
        let mut installer = Self::assemble(Scan::Pending(rx), WifiLink::spawn(), dark, false);
        installer.handoff = HandoffLink::system(HANDOFF_TARGET);
        installer.native = crate::install::NativeInstall::system();
        installer.firmware = FirmwareLink::system();
        installer.region = RegionLookup::Waiting;
        installer
    }
    pub fn with_hardware(hardware: HardwareSnapshot, dark: bool) -> Self {
        let wifi = WifiLink::offline("NetworkManager is not connected in this preview");
        Self::with_link(hardware, wifi, dark)
    }
    pub fn with_link(hardware: HardwareSnapshot, wifi: WifiLink, dark: bool) -> Self {
        Self::assemble(Scan::Done(Box::new(hardware)), wifi, dark, true)
    }
    fn assemble(scan: Scan, wifi: WifiLink, dark: bool, still: bool) -> Self {
        let catalog = Catalog::bundled().expect("bundled catalog is validated by tests");
        let mut selection = Selection::default();
        for family in Family::ALL {
            selection.ensure_defaults(&catalog, family);
        }
        LiveInstaller {
            native: crate::install::NativeInstall::with_service(std::sync::Arc::new(|_, _| {
                Err("borea-installd is not running in this preview".into())
            })),
            region: RegionLookup::Off,
            catalog,
            selection,
            dark,
            still,
            scan,
            wifi,
            form: WifiForm::default(),
            step: Step::Connect,
            reached: Step::Connect,
            picked_family: None,
            picked_distro: false,
            picked_edition: false,
            picked_variant: false,
            picked_mode: false,
            picked_compositor: false,
            picked_shell: false,
            firmware: FirmwareLink::preview(),
            firmware_dialog: false,
            handoff: HandoffLink::with_service(
                "omarchy",
                std::sync::Arc::new(|_, _| {
                    Err("borea-installd is not running in this preview".to_owned())
                }),
            ),
        }
    }
    pub fn set_handoff_service(&mut self, service: handoff::Service) {
        self.handoff = HandoffLink::with_service(HANDOFF_TARGET, service);
    }
    pub fn handoff(&self) -> &HandoffLink {
        &self.handoff
    }
    pub fn set_install_service(&mut self, service: handoff::Service) {
        self.native = crate::install::NativeInstall::with_service(service);
    }
    pub fn native(&self) -> &crate::install::NativeInstall {
        &self.native
    }
    fn native_desktop(&self) -> Option<borea_core::Desktop> {
        let c = self.selection.choice(self.family());
        if !(self.picked_edition
            && self.picked_compositor
            && self.picked_shell
            && matches!(self.distro().id.as_str(), "arch" | "cachyos")
            && c.mode == DesktopMode::Compose
            && c.shell == Some(ShellPick::Shell("noctalia".into())))
        {
            return None;
        }
        borea_core::Desktop::for_noctalia(c.compositor.as_deref()?)
    }
    fn native_target(&self) -> bool {
        self.native_desktop().is_some()
    }
    fn handoff_target(&self) -> Option<&'static str> {
        (self.picked_edition && self.distro().id.as_str() == self.handoff.target)
            .then_some(self.handoff.target)
    }
    pub fn step(&self) -> Step {
        self.step
    }
    pub fn selection(&self) -> &Selection {
        &self.selection
    }
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
    fn family(&self) -> Family {
        self.selection.selected
    }
    fn distro(&self) -> &Distro {
        self.selection.distro(&self.catalog, self.family())
    }
    fn edition(&self) -> &Edition {
        self.selection.edition(&self.catalog, self.family())
    }
    fn secure_boot(&self) -> Option<Readiness> {
        if self.native_target() {
            if !self.uefi() {
                return Some(Readiness::Unavailable("the native installer requires UEFI".into()));
            }
            return Some(if self.hardware()?.setup_mode == Fact::Known(true) {
                Readiness::Available { enrollment: secure_boot::Enrollment::Automatic }
            } else {
                Readiness::Unavailable("owner-key signing requires firmware Setup Mode; turn it on in UEFI setup or leave Secure Boot off".into())
            });
        }
        let spec = adapter(self.distro().id.as_str())?;
        Some(secure_boot::readiness(spec, self.hardware()?))
    }
    fn hardware(&self) -> Option<&HardwareSnapshot> {
        match &self.scan {
            Scan::Done(snapshot) => Some(snapshot),
            Scan::Pending(_) => None,
        }
    }
    fn poll(&mut self) {
        if let Scan::Pending(rx) = &self.scan
            && let Ok(snapshot) = rx.try_recv()
        {
            self.scan = Scan::Done(Box::new(snapshot));
        }
        self.wifi.poll();
        self.firmware.poll();
        self.handoff.poll();
        self.native.poll();
        let next = match &self.region {
            RegionLookup::Waiting if self.wifi.connected() => {
                let (tx, rx) = mpsc::channel();
                let asked = thread::Builder::new()
                    .name("region-lookup".into())
                    .spawn(move || {
                        let _ = tx.send(borea_net::geo_suggest());
                    })
                    .is_ok();
                Some(if asked { RegionLookup::Asking(rx) } else { RegionLookup::Done })
            }
            RegionLookup::Asking(rx) => match rx.try_recv() {
                Ok(answer) => {
                    if let Some(guess) = answer
                        .and_then(|g| borea_locale::guess(&g.country_code, g.time_zone.as_deref()))
                    {
                        self.native.suggest_region(guess);
                    }
                    Some(RegionLookup::Done)
                }
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(RegionLookup::Done),
            },
            _ => None,
        };
        if let Some(next) = next {
            self.region = next;
        }
        if self.step == Step::Review && self.native_target() {
            let extras = self
                .selection
                .ticked(&self.catalog, self.family())
                .iter()
                .map(|o| o.extra.id.clone())
                .collect();
            let net = self
                .wifi
                .networks
                .iter()
                .find(|n| n.active)
                .map(|n| n.ssid.clone())
                .map(|ssid| borea_core::NetChoice::Wifi { ssid })
                .unwrap_or(borea_core::NetChoice::Ethernet);
            self.native.target(self.distro().id.as_str() == "cachyos");
            if let Some(desktop) = self.native_desktop() {
                self.native.composition(desktop);
            }
            self.native.choices(extras, self.selection.secure_boot, net);
            self.native.load();
        }
        if self.step == Step::Review && self.handoff_target().is_some() {
            self.handoff.load();
        }
        self.automate();
    }
    fn automate(&mut self) {
        if self.step != Step::Connect || self.reached != Step::Connect {
            return;
        }
        // Already online (a cable, or a network joined here): move on by itself.
        if self.wifi.online() {
            self.go(Step::Machine);
            return;
        }
        // A joined network with an inconclusive internet probe needs neither
        // another passphrase nor an unrelated network selected automatically.
        if self.wifi.connected() {
            return;
        }
        // Offline: have the strongest network chosen and the cursor in its passphrase.
        if let Some(network) = self.wifi.networks.iter().find(|n| !n.active) {
            self.form.suggest(&network.ssid);
        }
    }
    pub fn suggested_network(&self) -> Option<&str> {
        self.form.picked()
    }
    fn go(&mut self, step: Step) {
        self.step = step;
        self.reached = self.reached.max(step);
    }
    pub fn can_continue(&self) -> bool {
        match self.step {
            Step::System => self.picked_family.is_some(),
            Step::Distribution => {
                self.picked_distro
                    && self.picked_edition
                    && (self.distro().variants.is_empty() || self.picked_variant)
            }
            Step::Desktop => self.desktop_picked(),
            Step::Review if self.handoff_target().is_some() => self.handoff.ready(),
            Step::Review if self.native_target() => false,
            Step::Review => Self::CAN_INSTALL,
            _ => true,
        }
    }
    fn desktop_picked(&self) -> bool {
        !self.edition().composable
            || (self.picked_mode
                && (self.selection.choice(self.family()).mode == DesktopMode::Bare
                    || (self.picked_compositor && self.picked_shell)))
    }
    fn reset_desktop(&mut self) {
        self.picked_mode = false;
        self.picked_compositor = false;
        self.picked_shell = false;
    }
    fn selection_title(&self) -> String {
        if self.picked_family.is_none() {
            "Choose a family".into()
        } else if !self.picked_distro {
            format!("{} · Choose a distribution", self.family())
        } else if !self.picked_edition {
            format!("{} · Choose an edition", self.distro().name)
        } else if !self.distro().variants.is_empty() && !self.picked_variant {
            format!("{} · {} · Choose an image variant", self.distro().name, self.edition().name)
        } else if !self.desktop_picked() {
            format!("{} · {} · Choose a desktop", self.distro().name, self.edition().name)
        } else {
            self.selection.title(&self.catalog, self.family())
        }
    }
    pub fn apply(&mut self, action: Action) {
        if action == Action::Next && !self.can_continue() {
            return;
        }
        // Once the hand-off runs, the machine is about to restart into it.
        if (self.handoff.busy() || self.native.busy())
            && !matches!(action, Action::ToggleTheme | Action::ToggleMotion)
        {
            return;
        }
        if action == Action::Next && self.step == Step::Review && self.handoff_target().is_some() {
            self.handoff.start();
            return;
        }
        if self.native.page == crate::install::Page::Finished
            && !matches!(
                action,
                Action::ToggleTheme
                    | Action::ToggleMotion
                    | Action::OpenFirmware
                    | Action::CancelFirmware
                    | Action::RestartFirmware
            )
        {
            return;
        }
        let catalog = &self.catalog;
        let s = &mut self.selection;
        let family = s.selected;
        // A changed target invalidates later review, including image variants.
        let resets = matches!(
            action,
            Action::Family(_) | Action::Distro(..) | Action::Edition(..) | Action::Variant(..)
        );
        // Composing from the Desktop step keeps you there, with Review to reach again.
        let recompose = matches!(
            action,
            Action::Compose(..)
                | Action::Mode(..)
                | Action::Compositor(..)
                | Action::Shell(..)
                | Action::Extra(..)
        );
        match action {
            Action::Go(step) if step <= self.reached => self.step = step,
            Action::Go(_) => {}
            Action::Next => {
                // Leaving the Desktop step with a composed desktop confirms it.
                if self.step == Step::Desktop
                    && s.composable(catalog, family)
                    && s.choice(family).mode == DesktopMode::Compose
                {
                    s.confirm(family);
                }
                if let Some(next) = self.step.next() {
                    self.go(next);
                }
            }
            Action::Back => {
                if let Some(back) = self.step.back() {
                    self.step = back;
                }
            }
            Action::Family(f) => {
                if f != Family::Arch {
                    return;
                }
                if self.picked_family != Some(f) {
                    s.set_distro(f, 0);
                    s.secure_boot = false;
                    self.picked_distro = false;
                    self.picked_edition = false;
                    self.picked_variant = false;
                    self.reset_desktop();
                } else {
                    s.select(f);
                }
                self.picked_family = Some(f);
                self.go(Step::Distribution);
            }
            Action::Distro(f, i) => {
                s.set_distro(f, i);
                s.secure_boot = false;
                self.picked_distro = true;
                self.picked_edition = false;
                self.picked_variant = false;
                self.reset_desktop();
            }
            Action::Edition(f, i) => {
                s.set_edition(f, i);
                self.picked_edition = true;
                self.picked_variant = false;
                self.reset_desktop();
            }
            Action::Variant(f, i) => {
                s.set_variant(f, i);
                self.picked_variant = true;
            }
            Action::Mode(f, mode) => {
                s.set_mode(f, mode);
                self.reset_desktop();
                self.picked_mode = true;
            }
            Action::Compositor(f, id) => {
                s.set_compositor(catalog, f, &id);
                self.picked_compositor = true;
                self.picked_shell = false;
            }
            Action::Shell(f, pick) => {
                s.set_shell(f, pick);
                self.picked_shell = true;
            }
            Action::Extra(f, id, on) => s.set_extra(f, &id, on),
            Action::Compose(f, target) => {
                s.compose_on(f, target);
                self.picked_distro = true;
                self.picked_edition = true;
                self.picked_variant = false;
                self.reset_desktop();
                self.picked_mode = true;
            }
            Action::SecureBoot(on) => s.secure_boot = on,
            Action::ToggleTheme => {
                self.dark = !self.dark;
            }
            Action::ToggleMotion => self.still = !self.still,
            Action::OpenFirmware => self.firmware_dialog = true,
            Action::CancelFirmware if !self.firmware.restarting => self.firmware_dialog = false,
            Action::CancelFirmware => {}
            Action::RestartFirmware => {
                if self.firmware_dialog && self.uefi() {
                    self.firmware.reboot();
                }
            }
            Action::HandoffChoose(pick) => self.handoff.choose(pick),
            Action::HandoffConfirm(on) => self.handoff.confirm(on),
            Action::HandoffReview => self.handoff.review(),
            Action::HandoffRetry => self.handoff.retry(),
        }
        // The hand-off adds the extras ticked for this card on first start.
        if self.handoff_target().is_some() {
            let family = self.family();
            let ticked = self.selection.ticked(&self.catalog, family);
            self.handoff.set_extras(ticked.iter().map(|o| o.extra.id.clone()).collect());
        }
        // Another target, or another edition, needs its disk chosen again.
        if resets {
            self.native.reset_storage();
            self.handoff.pick = None;
            self.handoff.confirmed = false;
            if matches!(self.handoff.phase, Phase::Reviewing | Phase::Reviewed(_)) {
                self.handoff.phase = Phase::Choosing;
            }
        }
        if resets {
            self.reached = self.reached.min(Step::Distribution);
        }
        if recompose {
            self.reached = self.reached.min(Step::Desktop);
        }
        // Secure Boot only stays chosen while the chosen target and machine allow it.
        if self.selection.secure_boot
            && !matches!(self.secure_boot(), Some(Readiness::Available { .. }))
        {
            self.selection.secure_boot = false;
        }
        for family in Family::ALL {
            self.selection.ensure_defaults(&self.catalog, family);
        }
        if self.native_target() {
            self.native.target(self.distro().id.as_str() == "cachyos");
            if let Some(desktop) = self.native_desktop() {
                self.native.composition(desktop);
            }
        }
    }
    fn uefi(&self) -> bool {
        self.hardware()
            .is_some_and(|hw| matches!(hw.firmware, Fact::Known(FirmwareMode::Uefi { .. })))
    }
    fn summary(&self, step: Step) -> Option<String> {
        if step > self.reached || step == self.step && step != Step::Review {
            return None;
        }
        let s = &self.selection;
        let f = self.family();
        match step {
            Step::Connect => Some(self.wifi.connection_label()),
            Step::Machine => self.hardware().map(|hw| {
                let level = hw.cpu.known().and_then(|c| c.x86_level).map(|l| l.to_string());
                format!("{} · {}", level.unwrap_or_else(|| "CPU".into()), hw.firmware)
            }),
            Step::System => self.picked_family.map(|f| f.to_string()),
            Step::Distribution => self
                .picked_edition
                .then(|| format!("{} · {}", self.distro().name, self.edition().name)),
            Step::Desktop => Some(match s.desktop_choice(&self.catalog, f) {
                None => "Included with the edition".into(),
                Some(_) => {
                    let title = s.title(&self.catalog, f);
                    let prefix = format!("{} · {} · ", self.distro().name, self.edition().name);
                    title.strip_prefix(&prefix).unwrap_or(&title).to_owned()
                }
            }),
            Step::Review => None,
        }
    }
    fn install_progress(&self) -> Option<f32> {
        if self.native_target()
            && matches!(
                self.native.page,
                crate::install::Page::Running | crate::install::Page::Finished
            )
        {
            return Some(self.native.progress);
        }
        match &self.handoff.phase {
            // Stages: find space, download, check, (make space,) start. The wind
            // keeps the highest value it has seen, so a new stage never slows it.
            Phase::Running { done, fraction, .. } => {
                Some(((done.len() as f32 + fraction.unwrap_or(0.0)) / 4.0).min(0.95))
            }
            Phase::Finished { .. } => Some(1.0),
            _ => None,
        }
    }
}
