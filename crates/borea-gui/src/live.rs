//! The Borea Live installer: Windpad's workflow as guided, full-screen steps.
//!
//! Same concepts and choices as Windpad: connect, read the machine, choose a family,
//! distribution, and edition, compose a desktop or keep none, then review. Rather
//! than one scrolling landing page, each step has its own screen, with a rail at
//! the side showing where you are and what you chose. Built for the live ISO:
//! large targets, keyboard friendly, nothing scrolls away.

use std::sync::mpsc::{self, Receiver};
use std::thread;

use borea_distro::adapter;
use borea_distro::secure_boot::{self, Readiness};
use borea_hardware::HardwareSnapshot;
use borea_live_catalog::{Catalog, Distro, Edition};
use borea_live_core::{Fact, Family, FirmwareMode};
use eframe::egui::{
    self, Align, CentralPanel, CornerRadius, Frame, Key, Layout, Margin, Panel, RichText,
    ScrollArea, Stroke, Ui,
};

use crate::firmware::{Choice, FirmwareLink};
use crate::handoff::{self, HandoffLink, Phase, Where};
use crate::icon;
use crate::model::{
    DesktopMode, Selection, ShellPick, capitalize, compose_teaser, device_name, family_line,
    hardware_facts,
};
use crate::theme::{self, DARK, LIGHT, Palette, RADIUS, size};
use crate::widgets::{
    self, badge, caption, choice_card, eyebrow, fact_icon, glyph, notice, primary, quiet, rows, tag,
};
use crate::wifi::{WifiForm, WifiLink};
use crate::wind;

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
    palette: Palette,
    still: bool,
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
}

impl LiveInstaller {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        thread::Builder::new()
            .name("live-scan".into())
            .spawn(move || {
                let _ = tx.send(borea_hardware::scan());
                repaint.request_repaint();
            })
            .expect("spawn hardware scan");
        let dark = ctx.system_theme() != Some(egui::Theme::Light);
        let mut installer =
            Self::assemble(&ctx, Scan::Pending(rx), WifiLink::spawn(ctx.clone()), dark, false);
        installer.handoff = HandoffLink::system("omarchy", ctx.clone());
        installer.firmware = FirmwareLink::system(ctx);
        installer
    }

    /// A still installer with fixed hardware and no network backend, for tests.
    pub fn with_hardware(ctx: &egui::Context, hardware: HardwareSnapshot, dark: bool) -> Self {
        let wifi = WifiLink::offline("NetworkManager is not connected in this preview");
        Self::with_link(ctx, hardware, wifi, dark)
    }

    /// A still installer with fixed hardware and a given network state, for tests.
    pub fn with_link(
        ctx: &egui::Context,
        hardware: HardwareSnapshot,
        wifi: WifiLink,
        dark: bool,
    ) -> Self {
        Self::assemble(ctx, Scan::Done(Box::new(hardware)), wifi, dark, true)
    }

    fn assemble(ctx: &egui::Context, scan: Scan, wifi: WifiLink, dark: bool, still: bool) -> Self {
        theme::install_fonts(ctx);
        let palette = if dark { DARK } else { LIGHT };
        theme::apply(ctx, &palette);
        let catalog = Catalog::bundled().expect("bundled catalog is validated by tests");
        let mut selection = Selection::default();
        for family in Family::ALL {
            selection.ensure_defaults(&catalog, family);
        }
        LiveInstaller {
            catalog,
            selection,
            palette,
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

    /// Answer hand-off requests with `service` instead of borea-installd (tests).
    pub fn set_handoff_service(&mut self, service: handoff::Service) {
        self.handoff = HandoffLink::with_service("omarchy", service);
    }

    pub fn handoff(&self) -> &HandoffLink {
        &self.handoff
    }

    /// The installer the chosen distribution hands off to, once it is chosen.
    fn handoff_target(&self) -> Option<&'static str> {
        (self.picked_edition && self.distro().id.as_str() == self.handoff.target)
            .then_some(self.handoff.target)
    }

    pub fn step(&self) -> Step {
        self.step
    }

    /// Whether this GUI can install. borea-installd can, through borea-cli; the
    /// Review step is not wired to it yet (docs/STATUS.md).
    pub const CAN_INSTALL: bool = false;

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

    /// Whether Secure Boot can be chosen for the selected target on this machine.
    fn secure_boot(&self) -> Option<Readiness> {
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
        if self.step == Step::Review && self.handoff_target().is_some() {
            self.handoff.load();
        }
        self.automate();
    }

    /// The parts of the first step nobody should have to do by hand. Only while the
    /// installer is still at its first step, so returning to Connect later stays put.
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

    pub fn apply(&mut self, ctx: &egui::Context, action: Action) {
        if action == Action::Next && !self.can_continue() {
            return;
        }
        // Once the hand-off runs, the machine is about to restart into it.
        if self.handoff.busy() && !matches!(action, Action::ToggleTheme | Action::ToggleMotion) {
            return;
        }
        if action == Action::Next && self.step == Step::Review && self.handoff_target().is_some() {
            self.handoff.start();
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
                self.palette = if self.palette.dark { LIGHT } else { DARK };
                theme::apply(ctx, &self.palette);
            }
            Action::ToggleMotion => self.still = !self.still,
            Action::OpenFirmware => self.firmware_dialog = true,
            Action::CancelFirmware if !self.firmware.restarting => self.firmware_dialog = false,
            Action::CancelFirmware => {}
            Action::RestartFirmware => {
                if self.firmware_dialog && self.uefi() {
                    self.firmware.reboot(ctx.clone());
                }
            }
            Action::HandoffChoose(pick) => self.handoff.choose(pick),
            Action::HandoffConfirm(on) => self.handoff.confirm(on),
            Action::HandoffReview => self.handoff.review(),
            Action::HandoffRetry => self.handoff.retry(),
        }
        // Another target, or another edition, needs its disk chosen again.
        if resets {
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
    }

    /// Render one frame and return what the user did.
    pub fn frame(&mut self, ui: &mut Ui) -> Vec<Action> {
        self.poll();
        let p = self.palette;
        let mut actions = Vec::new();
        let moving = !self.still && ui.input(|i| i.focused);
        let time = moving.then(|| ui.input(|i| i.time));

        Panel::left("rail")
            .exact_size(280.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                Frame::new()
                    .fill(p.paper)
                    .inner_margin(Margin::same(24))
                    .stroke(Stroke::new(1.0, p.line)),
            )
            .show(ui, |ui| self.rail(ui, time, &mut actions));

        Panel::bottom("navigation")
            .show_separator_line(false)
            .frame(Frame::new().fill(p.bg).inner_margin(Margin::symmetric(40, 18)))
            .show(ui, |ui| self.navigation(ui, &mut actions));

        let margin = Margin { left: 48, right: 40, top: 44, bottom: 8 };
        CentralPanel::no_frame().frame(Frame::new().fill(p.bg).inner_margin(margin)).show(
            ui,
            |ui| {
                // The wind runs across the full width of the step's header, margins included.
                let full = ui.max_rect() + margin;
                let band = egui::Rect::from_min_size(full.min, egui::vec2(full.width(), 230.0));
                wind::paint(
                    &ui.painter_at(band),
                    band.shrink2(egui::vec2(0.0, 10.0)),
                    p.accent,
                    time,
                );
                ui.label(eyebrow(
                    &format!("Step {} of {}", self.step.number(), Step::ALL.len()),
                    p.accent,
                ));
                ui.add_space(8.0);
                ui.label(RichText::new(self.step.title()).family(theme::display()).size(32.0));
                ui.label(caption(self.step.lead(), &p));
                ui.add_space(26.0);
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    // Leave room for the scroll bar, and keep lines readable on wide screens.
                    ui.set_max_width((ui.available_width() - 12.0).min(980.0));
                    match self.step {
                        Step::Connect => self.connect(ui),
                        Step::Machine => self.machine(ui),
                        Step::System => self.system(ui, &mut actions),
                        Step::Distribution => self.distribution(ui, &mut actions),
                        Step::Desktop => self.desktop(ui, &mut actions),
                        Step::Review => self.review(ui, &mut actions),
                    }
                    ui.add_space(24.0);
                });
            },
        );

        // Enter moves on and Escape goes back, unless a control has keyboard focus.
        let free = !self.firmware_dialog && ui.ctx().memory(|m| m.focused().is_none());
        if free && ui.input(|i| i.key_pressed(Key::Enter)) {
            actions.push(Action::Next);
        }
        if free && ui.input(|i| i.key_pressed(Key::Escape)) {
            actions.push(Action::Back);
        }

        if moving {
            ui.ctx().request_repaint_after(wind::FRAME);
        }
        self.firmware_confirmation(ui.ctx(), &mut actions);
        actions
    }

    // ── Rail ────────────────────────────────────────────────────────────────

    fn rail(&self, ui: &mut Ui, time: Option<f64>, actions: &mut Vec<Action>) {
        let p = self.palette;
        let (mark, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), egui::Sense::hover());
        wind::paint(&ui.painter_at(mark), mark, p.accent, time);
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::WIND).size(20.0).color(p.accent));
            let name = RichText::new("Borea").family(theme::display()).size(20.0);
            ui.label(name.extra_letter_spacing(3.5));
            ui.label(eyebrow("Live", p.sub));
        });
        ui.label(caption("Go wherever the wind takes you.", &p));
        if !Self::CAN_INSTALL && self.handoff_target().is_none() {
            ui.add_space(10.0);
            let text = RichText::new("Preview build · cannot install yet").size(size::SMALL);
            tag(ui, p.wash, Stroke::new(1.0, p.line), text);
        }
        ui.add_space(28.0);

        // Six steps must remain reachable on short laptop displays as well.
        ScrollArea::vertical()
            .id_salt("step-rail")
            .max_height((ui.available_height() - 176.0).max(100.0))
            .show(ui, |ui| {
                for step in Step::ALL {
                    let done = step < self.step || (step < self.reached && step != self.step);
                    let current = step == self.step;
                    let reachable = step <= self.reached;
                    let label = format!("Step {}: {}", step.number(), step.name());
                    let card = choice_card(
                        ui,
                        &p,
                        ("rail", step.number()),
                        &label,
                        current,
                        reachable,
                        |ui| {
                            ui.horizontal(|ui| {
                                let (dot, color) = match (current, done) {
                                    (true, _) => (icon::DOT, p.accent),
                                    (false, true) => (icon::CHECK, p.accent),
                                    (false, false) => (icon::RING, p.sub),
                                };
                                ui.label(RichText::new(dot).color(color));
                                ui.vertical(|ui| {
                                    let weight = if current { p.ink } else { p.sub };
                                    ui.label(
                                        RichText::new(step.name()).size(size::BODY).color(weight),
                                    );
                                    if let Some(summary) = self.summary(step) {
                                        ui.add(egui::Label::new(caption(summary, &p)).truncate());
                                    }
                                });
                            });
                        },
                    );
                    if card.clicked() {
                        actions.push(Action::Go(step));
                    }
                    ui.add_space(2.0);
                }
            });

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            let (enabled, hint) = self.firmware.control(self.uefi());
            let button = ui
                .add_enabled_ui(enabled, |ui| quiet(ui, &p, &FirmwareLink::label()))
                .inner
                .on_disabled_hover_text(hint)
                .on_hover_text(hint);
            if button.clicked() {
                actions.push(Action::OpenFirmware);
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let theme = if p.dark { "Light appearance" } else { "Dark appearance" };
                if quiet(ui, &p, &format!("{}  {theme}", icon::THEME)).clicked() {
                    actions.push(Action::ToggleTheme);
                }
                let motion = if self.still { "Wind paused" } else { "Pause wind" };
                if quiet(ui, &p, motion).clicked() {
                    actions.push(Action::ToggleMotion);
                }
            });
            ui.add_space(6.0);
            for line in self.status_lines().into_iter().rev() {
                ui.label(caption(line, &p));
            }
        });
    }

    fn uefi(&self) -> bool {
        self.hardware()
            .is_some_and(|hw| matches!(hw.firmware, Fact::Known(FirmwareMode::Uefi { .. })))
    }

    fn firmware_confirmation(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        if !self.firmware_dialog {
            return;
        }
        match self.firmware.confirmation(ctx, &self.palette) {
            Some(Choice::Cancel) => actions.push(Action::CancelFirmware),
            Some(Choice::Restart) => actions.push(Action::RestartFirmware),
            None => {}
        }
    }

    /// What each completed step decided, shown under its name in the rail.
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

    fn status_lines(&self) -> Vec<String> {
        let symbol = if self.wifi.connected() { icon::WIFI } else { icon::WIFI_OFF };
        let wifi = format!("{symbol}  {}", self.wifi.connection_label());
        let mut lines = vec![wifi];
        if let Some(hw) = self.hardware() {
            if let Some(cpu) = hw.cpu.known() {
                lines.push(cpu.model.clone().unwrap_or_else(|| "Unknown CPU".into()));
            }
            lines.push(format!("{} · {}", hw.firmware, device_name(hw)));
        } else {
            lines.push("Scanning hardware…".into());
        }
        lines
    }

    fn navigation(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        ui.horizontal(|ui| {
            ui.label(caption(self.selection_title(), &p));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let next = match self.step {
                    Step::Review => "Begin installation".to_owned(),
                    Step::Desktop => format!("Review  {}", icon::NEXT),
                    _ => format!("Next  {}", icon::NEXT),
                };
                let ready = self.can_continue();
                let button = ui.add_enabled_ui(ready, |ui| primary(ui, &p, &next, false)).inner;
                if button.clicked() {
                    actions.push(Action::Next);
                }
                if !ready && self.step == Step::Review {
                    let why = match (self.handoff_target(), &self.handoff.phase) {
                        (None, _) => "Not in this preview build",
                        (Some(_), Phase::Running { .. } | Phase::Finished { .. }) => "Under way",
                        (Some(_), Phase::Reviewed(_)) => "Confirm shrinking Windows first",
                        (Some(_), _) => "Choose where it goes, then check the plan",
                    };
                    ui.label(caption(why, &p));
                }
                let back = format!("{}  Back", icon::BACK);
                if self.step.back().is_some()
                    && !self.handoff.busy()
                    && quiet(ui, &p, &back).clicked()
                {
                    actions.push(Action::Back);
                }
            });
        });
    }

    // ── Steps ───────────────────────────────────────────────────────────────

    fn connect(&mut self, ui: &mut Ui) {
        let p = self.palette;
        if self.wifi.online() {
            let through =
                self.wifi.status.as_ref().and_then(|s| s.primary.clone()).unwrap_or_default();
            notice(
                ui,
                &p,
                &format!("You're online through {through}. Continue whenever you're ready."),
            );
            ui.add_space(12.0);
        }
        ui.set_max_width(620.0);
        let LiveInstaller { wifi, form, .. } = self;
        if let Some(command) = crate::wifi::panel(ui, &p, wifi, form) {
            wifi.send(command);
        }
        if !self.wifi.online() {
            ui.add_space(10.0);
            let hint = if self.wifi.connected() {
                "Your network is connected. Internet access is not confirmed; a sign-in page or a blocked connectivity check may be the reason."
            } else {
                "Wired networks connect on their own. Without a connection you can still look around, but nothing can be downloaded."
            };
            ui.label(caption(hint, &p));
        }
    }

    fn machine(&self, ui: &mut Ui) {
        let p = self.palette;
        let Some(hw) = self.hardware() else {
            ui.label(caption("Scanning…", &p));
            return;
        };
        let facts = hardware_facts(hw);
        grid(ui, 3, &facts, |ui, (label, value, detail)| {
            info_card(ui, &p, label, value, detail);
        });
        widgets::findings(ui, &p, hw);
    }

    fn system(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        let catalog = &self.catalog;
        section(ui, &p, "Family");
        grid(ui, 4, &Family::ALL, |ui, f| {
            let names: Vec<_> = catalog.family(*f).map(|d| d.name.as_str()).collect();
            let card = choice_card(
                ui,
                &p,
                ("family", *f),
                &format!("{f} family"),
                self.picked_family == Some(*f),
                true,
                |ui| {
                    ui.set_min_height(124.0);
                    ui.horizontal(|ui| {
                        glyph(ui, icon::family(*f), 22.0, p.accent);
                        ui.label(
                            RichText::new(f.to_string())
                                .family(theme::display())
                                .size(size::HEADING),
                        );
                    });
                    ui.label(caption(family_line(*f), &p));
                    ui.label(RichText::new(names.join(" · ")).size(size::TINY).color(p.sub));
                },
            );
            if card.clicked() {
                actions.push(Action::Family(*f));
            }
        });
    }

    fn distribution(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        let catalog = &self.catalog;
        let s = &self.selection;
        let family = self.family();
        let choice = s.choice(family);
        section(ui, &p, &format!("{family} distributions"));
        let distros: Vec<(usize, &Distro)> = catalog.family(family).enumerate().collect();
        grid(ui, 3, &distros, |ui, (i, d)| {
            let card = choice_card(
                ui,
                &p,
                ("distro", &d.id),
                &d.name,
                self.picked_distro && *i == choice.distro,
                true,
                |ui| {
                    ui.set_min_height(98.0);
                    ui.horizontal(|ui| {
                        glyph(ui, icon::distro(d.id.as_str()), size::HEADING, p.accent);
                        ui.label(RichText::new(&d.name).size(size::HEADING - 2.0));
                    });
                    ui.label(caption(&d.summary, &p));
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(capitalize(&d.channel.to_string()))
                                .size(size::TINY)
                                .color(p.accent),
                        );
                        let status = capitalize(&d.status.to_string());
                        ui.label(caption(format!("· {status} integration"), &p));
                    });
                },
            );
            if card.clicked() {
                actions.push(Action::Distro(family, *i));
            }
        });

        if !self.picked_distro {
            return;
        }
        let distro = s.distro(catalog, family);
        ui.add_space(8.0);
        match &distro.source {
            Some(source) => {
                ui.hyperlink_to("Upstream project", source);
            }
            None => notice(
                ui,
                &p,
                "Upstream source pending verification. These editions are requested options, not verified downloads.",
            ),
        }
        if family == Family::Opensuse {
            ui.label(caption("Choose an edition, then add Gaming, Media codecs, Flatpak or Printing on the next page.", &p));
        }
        section(ui, &p, "Edition");
        let editions: Vec<(usize, &Edition)> = distro.editions.iter().enumerate().collect();
        grid(ui, 4, &editions, |ui, (i, e)| {
            let card = choice_card(
                ui,
                &p,
                ("edition", &e.id),
                &e.name,
                self.picked_edition && *i == choice.edition,
                true,
                |ui| {
                    ui.set_min_height(64.0);
                    ui.label(RichText::new(&e.name).size(size::BODY + 1.0));
                    let note = match (e.composable, e.device) {
                        (true, _) => "No desktop · compose your own",
                        (false, Some(_)) => "Made for handhelds",
                        _ => "Desktop included",
                    };
                    ui.label(caption(note, &p));
                },
            );
            if card.clicked() {
                actions.push(Action::Edition(family, *i));
            }
        });

        if self.picked_edition && !distro.variants.is_empty() {
            section(ui, &p, "Image variant");
            let variants: Vec<(usize, &String)> = distro.variants.iter().enumerate().collect();
            grid(ui, 4, &variants, |ui, (i, v)| {
                if choice_card(
                    ui,
                    &p,
                    ("variant", *i),
                    v,
                    self.picked_variant && *i == choice.variant,
                    true,
                    |ui| {
                        ui.label(RichText::new(v.as_str()).size(size::BODY));
                    },
                )
                .clicked()
                {
                    actions.push(Action::Variant(family, *i));
                }
            });
        }
    }

    /// The desktop, then the extras, which every edition has.
    fn desktop(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        let family = self.family();
        self.compose(ui, actions);
        section(ui, &p, "Extras");
        if self.catalog.extras.on(&self.distro().id).next().is_none() {
            ui.label(caption(
                "Extra packages will be offered after the upstream repositories are verified.",
                &p,
            ));
        }
        let ticked = |id: &str| self.selection.extra(&self.catalog, family, id);
        if let Some((id, on)) = widgets::extras(ui, &p, &self.catalog, self.distro(), ticked) {
            actions.push(Action::Extra(family, id, on));
        }
    }

    fn compose(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        let catalog = &self.catalog;
        let s = &self.selection;
        let family = self.family();
        let distro = self.distro();
        let edition = self.edition();
        let choice = s.choice(family);

        if distro.source.is_none() {
            notice(
                ui,
                &p,
                "This project's upstream downloads and editions still need verification.",
            );
            return;
        }
        if !edition.composable {
            notice(
                ui,
                &p,
                &format!(
                    "{} {} comes with its own desktop. Borea installs it as upstream ships it.",
                    distro.name, edition.name
                ),
            );
            if let Some(target) = s.compose_target(catalog, family) {
                let on = catalog.family(family).nth(target.0).expect("target indexes the family");
                let title = format!("Or compose your own desktop on {} Minimal", on.name);
                ui.add_space(12.0);
                ui.set_max_width(660.0);
                let teaser = compose_teaser(catalog, on);
                if widgets::compose_offer(ui, &p, "compose-offer", &title, &teaser).clicked() {
                    actions.push(Action::Compose(family, target));
                }
            }
            return;
        }

        let modes = [
            (DesktopMode::Compose, "Compose a desktop", "A compositor, and a shell to live on it."),
            (
                DesktopMode::Bare,
                "No desktop",
                "A console login, nothing extra. Add a desktop any time later.",
            ),
        ];
        grid(ui, 2, &modes, |ui, (mode, name, about)| {
            if choice_card(
                ui,
                &p,
                ("mode", *name),
                name,
                self.picked_mode && choice.mode == *mode,
                true,
                |ui| {
                    ui.label(RichText::new(*name).size(size::HEADING - 2.0));
                    ui.label(caption(*about, &p));
                },
            )
            .clicked()
            {
                actions.push(Action::Mode(family, *mode));
            }
        });
        if self.picked_mode && choice.mode == DesktopMode::Bare {
            ui.add_space(6.0);
            ui.label(caption(s.bare_summary(catalog, family), &p));
        }
        if !self.picked_mode || choice.mode == DesktopMode::Bare {
            return;
        }

        section(ui, &p, &format!("{} Compositor", icon::ONE));
        let compositors: Vec<_> = catalog.desktops.compositors(&distro.id).collect();
        grid(ui, 3, &compositors, |ui, offer| {
            let c = offer.item;
            let on = self.picked_compositor && choice.compositor.as_deref() == Some(c.id.as_str());
            let card =
                choice_card(ui, &p, ("compositor", &c.id), &c.name, on, offer.available(), |ui| {
                    ui.set_min_height(92.0);
                    ui.horizontal(|ui| {
                        glyph(ui, icon::compositor(&c.id), size::HEADING, p.accent);
                        ui.label(RichText::new(&c.name).size(size::HEADING - 2.0));
                    });
                    ui.label(caption(&c.style, &p));
                    match offer.source {
                        Some(source) => badge(ui, &p, source),
                        None => {
                            ui.label(caption(format!("Not packaged for {}", distro.name), &p));
                        }
                    }
                });
            if card.clicked() {
                actions.push(Action::Compositor(family, c.id.clone()));
            }
        });

        if !self.picked_compositor {
            return;
        }
        section(ui, &p, &format!("{} Shell", icon::TWO));
        let compositor = choice.compositor.clone().unwrap_or_default();
        let compositor_name = s.compositor_name(catalog, family).unwrap_or_default().to_owned();
        let mut shells: Vec<Option<borea_live_catalog::Offer<'_, borea_live_catalog::Shell>>> =
            catalog.desktops.ranked(&distro.id, &compositor).map(Some).collect();
        shells.push(None);
        let suggested = catalog.desktops.suggested(&distro.id, &compositor).map(|s| s.id.clone());
        grid(ui, 2, &shells, |ui, offer| match offer {
            Some(offer) => {
                let shell = offer.item;
                let on =
                    self.picked_shell && choice.shell == Some(ShellPick::Shell(shell.id.clone()));
                let card = choice_card(ui, &p, ("shell", &shell.id), &shell.name, on, true, |ui| {
                    ui.set_min_height(96.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&shell.name).size(size::BODY + 1.0));
                        ui.label(caption(&shell.by, &p));
                    });
                    if suggested.as_ref() == Some(&shell.id) {
                        let why = if shell.made_for(&distro.id) {
                            format!("{} Made for {}", icon::STAR, distro.name)
                        } else {
                            format!("{} Suggested", icon::STAR)
                        };
                        ui.label(RichText::new(why).size(size::TINY).color(p.accent));
                    }
                    ui.label(caption(&shell.summary, &p));
                    ui.horizontal(|ui| {
                        if let Some(source) = offer.source {
                            badge(ui, &p, source);
                        }
                        ui.label(caption(format!("{} · {}", shell.framework, shell.license), &p));
                    });
                });
                if card.clicked() {
                    actions.push(Action::Shell(family, ShellPick::Shell(shell.id.clone())));
                }
            }
            None => {
                let on = self.picked_shell && choice.shell == Some(ShellPick::NoShell);
                let card = choice_card(ui, &p, "no-shell", "No shell", on, true, |ui| {
                    ui.set_min_height(96.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("No shell").size(size::BODY + 1.0));
                        ui.label(caption(format!("just {compositor_name}"), &p));
                    });
                    ui.label(caption(
                        format!("{compositor_name} on its own, with its default config. Bring your own bar and launcher later."),
                        &p,
                    ));
                });
                if card.clicked() {
                    actions.push(Action::Shell(family, ShellPick::NoShell));
                }
            }
        });
        ui.add_space(6.0);
        if self.picked_shell {
            ui.label(caption(s.arrival(catalog, family), &p));
        }
    }

    fn review(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        let s = &self.selection;
        let family = self.family();
        if self.handoff_target().is_some() {
            ui.label(RichText::new(s.title(&self.catalog, family)).size(size::TITLE));
            ui.add_space(12.0);
            self.handoff_review(ui, actions);
            return;
        }
        if !Self::CAN_INSTALL {
            ui.set_max_width(660.0);
            notice(
                ui,
                &p,
                "This is a preview build: it cannot install yet. The review below is what Borea would install; nothing on this machine changes.",
            );
            ui.add_space(12.0);
        }
        if self.distro().source.is_none() {
            notice(
                ui,
                &p,
                "Upstream source pending verification. No download or installation recipe is available for this project yet.",
            );
            ui.add_space(12.0);
        }
        ui.label(RichText::new(s.title(&self.catalog, family)).size(size::TITLE));
        ui.add_space(12.0);
        Frame::new()
            .fill(p.paper)
            .stroke(Stroke::new(1.0, p.line))
            .corner_radius(CornerRadius::same(RADIUS))
            .inner_margin(Margin::same(20))
            .show(ui, |ui| {
                ui.set_max_width(620.0);
                rows(ui, &p, &s.rows(&self.catalog, family));
            });
        self.secure_boot_choice(ui, actions);
        ui.add_space(12.0);
        ui.set_max_width(660.0);
        notice(
            ui,
            &p,
            "Next comes storage: you pick the disk to use, and nothing is ever preselected. Then account settings, and one final review before anything is written.",
        );
    }

    /// Omarchy's Review: where it goes, Borea's plan, then the hand-off itself.
    fn handoff_review(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        let link = &self.handoff;
        ui.set_max_width(660.0);
        match &link.phase {
            Phase::Running { stage, fraction, lines, choose } => {
                section(ui, &p, "Getting Omarchy ready");
                ui.label(RichText::new(stage).size(size::BODY + 2.0));
                ui.add_space(8.0);
                let bar =
                    egui::ProgressBar::new(fraction.unwrap_or(0.0)).animate(fraction.is_none());
                ui.add(if fraction.is_some() { bar.show_percentage() } else { bar });
                for line in lines {
                    ui.label(caption(line, &p));
                }
                ui.add_space(12.0);
                notice(ui, &p, &format!("Then: {choose}"));
                return;
            }
            Phase::Finished { choose } => {
                section(ui, &p, "Restarting into Omarchy's installer");
                notice(ui, &p, choose);
                ui.label(caption(
                    "The screen goes dark for a moment while Omarchy's installer starts.",
                    &p,
                ));
                return;
            }
            _ => {}
        }
        notice(
            ui,
            &p,
            "Omarchy installs with its own installer. Borea gets it ready: it downloads Omarchy and checks its signature, makes room beside Windows if you choose to keep it, then restarts straight into Omarchy's installer. Nothing changes until you begin.",
        );
        if let Some(hw) = self.hardware()
            && hw.secure_boot == Fact::Known(true)
        {
            ui.add_space(8.0);
            let text = RichText::new("Omarchy needs Secure Boot off. Turn it off in UEFI setup (the control at the bottom of the side panel) before installing.").size(size::SMALL);
            tag(ui, p.wash, Stroke::new(1.0, p.accent), text);
        }
        section(ui, &p, "Where Omarchy goes");
        match &link.phase {
            Phase::Idle | Phase::Loading => {
                ui.label(caption("Looking at the disks…", &p));
                return;
            }
            Phase::Failed(why) if link.disks.is_empty() => {
                notice(ui, &p, why);
                if quiet(ui, &p, "Try again").clicked() {
                    actions.push(Action::HandoffRetry);
                }
                return;
            }
            _ => {}
        }
        let mut offered = false;
        for (i, disk) in link.disks.iter().enumerate().filter(|(_, d)| d.usable()) {
            if disk.size() < 32 * handoff::GIB {
                continue;
            }
            offered = true;
            let whole = link.pick == Some(Where::Whole { disk: i });
            let label = format!("Erase {}", disk.name());
            let card = choice_card(ui, &p, ("handoff-whole", i), &label, whole, true, |ui| {
                ui.label(RichText::new(format!("{}  {label}", icon::DISK)).size(size::BODY));
                ui.label(caption(
                    format!("Omarchy takes the whole disk ({}). Everything on it is erased by Omarchy's installer, after its own confirmation.", disk.path()),
                    &p,
                ));
            });
            if card.clicked() {
                actions.push(Action::HandoffChoose(Where::Whole { disk: i }));
            }
            ui.add_space(4.0);
            for (j, part) in disk.parts.iter().enumerate() {
                let Some(max) =
                    part.max_linux_size.filter(|m| part.fs == "ntfs" && *m >= 32 * handoff::GIB)
                else {
                    continue;
                };
                let size = match link.pick {
                    Some(Where::Alongside { disk, part, size }) if disk == i && part == j => {
                        Some(size)
                    }
                    _ => None,
                };
                let label = format!("Beside Windows on {}", disk.name());
                let card = choice_card(
                    ui,
                    &p,
                    ("handoff-beside", i, j),
                    &label,
                    size.is_some(),
                    true,
                    |ui| {
                        ui.label(
                            RichText::new(format!("{}  {label}", icon::DISK)).size(size::BODY),
                        );
                        ui.label(caption(
                        format!("Borea shrinks Windows ({}) to make room; Omarchy's installer uses only that space. Windows keeps everything else.", part.path),
                        &p,
                    ));
                    },
                );
                if card.clicked() && size.is_none() {
                    let size = (60 * handoff::GIB).min(max) / handoff::GIB * handoff::GIB;
                    actions.push(Action::HandoffChoose(Where::Alongside {
                        disk: i,
                        part: j,
                        size,
                    }));
                }
                if let Some(size) = size {
                    let mut gib = size / handoff::GIB;
                    let slider = egui::Slider::new(&mut gib, 32..=max / handoff::GIB)
                        .suffix(" GiB")
                        .text("for Omarchy");
                    if ui.add(slider).changed() {
                        actions.push(Action::HandoffChoose(Where::Alongside {
                            disk: i,
                            part: j,
                            size: gib * handoff::GIB,
                        }));
                    }
                }
                ui.add_space(4.0);
            }
        }
        if !offered {
            notice(
                ui,
                &p,
                "No disk here can take Omarchy: it needs a disk of at least 32 GiB that is not the Borea stick and not in use.",
            );
            return;
        }
        ui.add_space(10.0);
        match &link.phase {
            Phase::Reviewing => {
                ui.label(caption("Checking the plan…", &p));
            }
            Phase::Reviewed(review) => {
                section(ui, &p, "Borea's plan");
                Frame::new()
                    .fill(p.paper)
                    .stroke(Stroke::new(1.0, p.line))
                    .corner_radius(CornerRadius::same(RADIUS))
                    .inner_margin(Margin::same(20))
                    .show(ui, |ui| {
                        for step in &review.steps {
                            ui.label(
                                RichText::new(format!("{}  {step}", icon::CHECK)).size(size::SMALL),
                            );
                        }
                    });
                ui.add_space(8.0);
                notice(ui, &p, &format!("Then: {}", review.choose));
                if matches!(link.pick, Some(Where::Alongside { .. })) {
                    ui.add_space(8.0);
                    let mut on = link.confirmed;
                    if ui
                        .checkbox(
                            &mut on,
                            "Shrink Windows now. I have a backup of anything important on it.",
                        )
                        .changed()
                    {
                        actions.push(Action::HandoffConfirm(on));
                    }
                }
                ui.add_space(6.0);
                ui.label(caption("Begin installation starts it; the machine then restarts into Omarchy's installer.", &p));
            }
            phase => {
                if let Phase::Failed(why) = phase {
                    notice(ui, &p, why);
                    ui.add_space(8.0);
                }
                let ready = link.pick.is_some();
                if ui
                    .add_enabled_ui(ready, |ui| primary(ui, &p, "Check this plan", false))
                    .inner
                    .clicked()
                {
                    actions.push(Action::HandoffReview);
                }
            }
        }
    }

    /// Secure Boot for the new system: off unless chosen, with every step shown first.
    fn secure_boot_choice(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        let Some(hw) = self.hardware() else { return };
        section(ui, &p, "Secure Boot");
        Frame::new()
            .fill(p.paper)
            .stroke(Stroke::new(1.0, p.line))
            .corner_radius(CornerRadius::same(RADIUS))
            .inner_margin(Margin::same(20))
            .show(ui, |ui| {
                ui.set_max_width(620.0);
                let state = |fact: &Fact<bool>| match fact {
                    Fact::Known(true) => "on",
                    Fact::Known(false) => "off",
                    Fact::Unknown { .. } => "unknown",
                };
                ui.label(caption(
                    format!(
                        "This machine: Secure Boot {} · Setup Mode {}",
                        state(&hw.secure_boot),
                        state(&hw.setup_mode)
                    ),
                    &p,
                ));
                ui.add_space(6.0);
                let readiness = self.secure_boot();
                if ui.radio(!self.selection.secure_boot, "Leave Secure Boot off").clicked() {
                    actions.push(Action::SecureBoot(false));
                }
                let available = matches!(readiness, Some(Readiness::Available { .. }));
                if ui.add_enabled(available, egui::RadioButton::new(self.selection.secure_boot, "Turn Secure Boot on")).clicked() {
                    actions.push(Action::SecureBoot(true));
                }
                if !self.selection.secure_boot {
                    ui.label(caption(
                        "Install without Secure Boot. No key enrollment is required; this also works on UEFI machines without Secure Boot support.",
                        &p,
                    ));
                }
                match readiness {
                    Some(Readiness::Available { enrollment }) => {
                        ui.add_space(4.0);
                        if self.selection.secure_boot {
                            for step in secure_boot::STEPS {
                                ui.label(
                                    RichText::new(format!("{}  {step}", icon::CHECK))
                                        .size(size::SMALL),
                                );
                            }
                            ui.add_space(4.0);
                            ui.label(caption(enrollment.describe(), &p));
                        }
                    }
                    Some(Readiness::Unavailable(why)) => {
                        ui.label(caption(format!("Signed boot is not available here: {why}."), &p));
                    }
                    None => {}
                }
                if let Some(warning) = secure_boot::conflict(self.selection.secure_boot, hw) {
                    ui.add_space(6.0);
                    let text = RichText::new(warning).size(size::SMALL);
                    tag(ui, p.wash, Stroke::new(1.0, p.accent), text);
                    ui.label(caption("Use the CPU icon in the footer to restart into UEFI setup.", &p));
                }
            });
    }
}

impl eframe::App for LiveInstaller {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for action in self.frame(ui) {
            self.apply(&ctx, action);
        }
    }
}

// ── Pieces ──────────────────────────────────────────────────────────────────

fn section(ui: &mut Ui, p: &Palette, title: &str) {
    ui.add_space(16.0);
    ui.label(eyebrow(title, p.sub));
    ui.add_space(6.0);
}

/// Lay items out in rows of `cols` equal columns, fewer when the window is narrow.
fn grid<T>(ui: &mut Ui, cols: usize, items: &[T], mut cell: impl FnMut(&mut Ui, &T)) {
    let cols = cols.min((ui.available_width() / 220.0).floor().max(1.0) as usize).max(1);
    for row in items.chunks(cols) {
        ui.columns(cols, |columns| {
            for (column, item) in columns.iter_mut().zip(row) {
                cell(column, item);
            }
        });
        ui.add_space(4.0);
    }
}

fn info_card(ui: &mut Ui, p: &Palette, label: &str, value: &str, detail: &str) {
    Frame::new()
        .fill(p.paper)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height(72.0);
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.horizontal(|ui| {
                    glyph(ui, fact_icon(label), size::SMALL, p.accent);
                    ui.label(eyebrow(label, p.sub));
                });
                ui.add(egui::Label::new(RichText::new(value).size(size::BODY)).truncate());
                if !detail.is_empty() {
                    ui.add(egui::Label::new(caption(detail, p)).truncate());
                }
            });
        });
}
