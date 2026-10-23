//! The Borea Live installer: Windpad's workflow as guided, full-screen steps.
//!
//! Same concepts and choices as Windpad: connect, read the machine, choose a family,
//! distribution, and edition, compose a desktop or keep none, then review. Rather
//! than one scrolling landing page, each step has its own screen, with a rail at
//! the side showing where you are and what you chose. Built for the live ISO:
//! large targets, keyboard friendly, nothing scrolls away.

use std::sync::mpsc::{self, Receiver};
use std::thread;

use borea_catalog::{Catalog, Distro, Edition};
use borea_core::{Fact, Family};
use borea_distro::secure_boot::{self, Readiness};
use borea_distro::{Finding, adapter, machine_findings};
use borea_hardware::{DiskUsage, HardwareSnapshot};
use eframe::egui::{
    self, Align, CentralPanel, CornerRadius, Frame, Key, Layout, Margin, Panel, RichText,
    ScrollArea, Stroke, Ui,
};

use crate::icon;
use crate::model::{DesktopMode, Selection, ShellPick, capitalize, compose_teaser, device_name};
use crate::theme::{self, DARK, LIGHT, Palette, RADIUS, size};
use crate::widgets::{
    self, badge, caption, choice_card, eyebrow, glyph, notice, primary, quiet, rows, segmented, tag,
};
use crate::wifi::{WifiForm, WifiLink};
use crate::wind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    Connect,
    Machine,
    System,
    Desktop,
    Review,
}

impl Step {
    pub const ALL: [Step; 5] =
        [Step::Connect, Step::Machine, Step::System, Step::Desktop, Step::Review];

    fn number(self) -> usize {
        Step::ALL.iter().position(|s| *s == self).unwrap_or(0) + 1
    }

    fn name(self) -> &'static str {
        match self {
            Step::Connect => "Connect",
            Step::Machine => "Machine",
            Step::System => "System",
            Step::Desktop => "Desktop",
            Step::Review => "Review",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Step::Connect => "Catch a connection.",
            Step::Machine => "A sense of your system.",
            Step::System => "Choose your next direction.",
            Step::Desktop => "Compose your desktop.",
            Step::Review => "Ready when you are.",
        }
    }

    fn lead(self) -> &'static str {
        match self {
            Step::Connect => "Borea downloads your system as it installs. Connect once, here.",
            Step::Machine => "Read from this machine, read-only. Unknown stays unknown.",
            Step::System => {
                "A family, a distribution, an edition. Each keeps its own release and updates."
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
        Self::assemble(&ctx, Scan::Pending(rx), WifiLink::spawn(ctx.clone()), dark, false)
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
        }
    }

    pub fn step(&self) -> Step {
        self.step
    }

    /// Whether this build can install. Until the planner and privileged worker
    /// exist it cannot, and every screen that leads there says so first.
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

    pub fn apply(&mut self, ctx: &egui::Context, action: Action) {
        let catalog = &self.catalog;
        let s = &mut self.selection;
        let family = s.selected;
        // Changing the system after reaching later steps means reviewing them again.
        let resets = matches!(action, Action::Family(_) | Action::Distro(..) | Action::Edition(..));
        // Composing from the Desktop step keeps you there, with Review to reach again.
        let recompose = matches!(action, Action::Compose(..));
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
            Action::Family(f) => s.select(f),
            Action::Distro(f, i) => s.set_distro(f, i),
            Action::Edition(f, i) => s.set_edition(f, i),
            Action::Variant(f, i) => s.set_variant(f, i),
            Action::Mode(f, mode) => s.set_mode(f, mode),
            Action::Compositor(f, id) => s.set_compositor(catalog, f, &id),
            Action::Shell(f, pick) => s.set_shell(f, pick),
            Action::Extra(f, id, on) => s.set_extra(f, &id, on),
            Action::Compose(f, target) => s.compose_on(f, target),
            Action::SecureBoot(on) => s.secure_boot = on,
            Action::ToggleTheme => {
                self.palette = if self.palette.dark { LIGHT } else { DARK };
                theme::apply(ctx, &self.palette);
            }
            Action::ToggleMotion => self.still = !self.still,
        }
        if resets {
            self.reached = self.reached.min(Step::System);
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
                        Step::Desktop => self.desktop(ui, &mut actions),
                        Step::Review => self.review(ui, &mut actions),
                    }
                    ui.add_space(24.0);
                });
            },
        );

        // Enter moves on and Escape goes back, unless a control has keyboard focus.
        let free = ui.ctx().memory(|m| m.focused().is_none());
        if free && ui.input(|i| i.key_pressed(Key::Enter)) {
            actions.push(Action::Next);
        }
        if free && ui.input(|i| i.key_pressed(Key::Escape)) {
            actions.push(Action::Back);
        }

        if moving {
            ui.ctx().request_repaint_after(wind::FRAME);
        }
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
            let name = RichText::new("BOREA").family(theme::display()).size(20.0);
            ui.label(name.extra_letter_spacing(3.5));
            ui.label(eyebrow("Live", p.sub));
        });
        ui.label(caption("Go wherever the wind takes you.", &p));
        if !Self::CAN_INSTALL {
            ui.add_space(10.0);
            let text = RichText::new("Preview build · cannot install yet").size(size::SMALL);
            tag(ui, p.wash, Stroke::new(1.0, p.line), text);
        }
        ui.add_space(28.0);

        for step in Step::ALL {
            let done = step < self.step || (step < self.reached && step != self.step);
            let current = step == self.step;
            let reachable = step <= self.reached;
            let label = format!("Step {}: {}", step.number(), step.name());
            let card =
                choice_card(ui, &p, ("rail", step.number()), &label, current, reachable, |ui| {
                    ui.horizontal(|ui| {
                        let (dot, color) = match (current, done) {
                            (true, _) => (icon::DOT, p.accent),
                            (false, true) => (icon::CHECK, p.accent),
                            (false, false) => (icon::RING, p.sub),
                        };
                        ui.label(RichText::new(dot).color(color));
                        ui.vertical(|ui| {
                            let weight = if current { p.ink } else { p.sub };
                            ui.label(RichText::new(step.name()).size(size::BODY).color(weight));
                            if let Some(summary) = self.summary(step) {
                                ui.add(egui::Label::new(caption(summary, &p)).truncate());
                            }
                        });
                    });
                });
            if card.clicked() {
                actions.push(Action::Go(step));
            }
            ui.add_space(2.0);
        }

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.horizontal(|ui| {
                let theme = if p.dark { "Light" } else { "Dark" };
                if quiet(ui, &p, theme).clicked() {
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

    /// What each completed step decided, shown under its name in the rail.
    fn summary(&self, step: Step) -> Option<String> {
        if step > self.reached || step == self.step && step != Step::Review {
            return None;
        }
        let s = &self.selection;
        let f = self.family();
        match step {
            Step::Connect => {
                Some(if self.wifi.online() { "Online".into() } else { "Not connected".into() })
            }
            Step::Machine => self.hardware().map(|hw| {
                let level = hw.cpu.known().and_then(|c| c.x86_level).map(|l| l.to_string());
                format!("{} · {}", level.unwrap_or_else(|| "CPU".into()), hw.firmware)
            }),
            Step::System => Some(format!("{} · {}", self.distro().name, self.edition().name)),
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
        let wifi = match (&self.wifi.unavailable, self.wifi.online()) {
            (Some(_), _) => "Wi-Fi unavailable".to_owned(),
            (None, true) => format!("{}  Online", icon::WIFI),
            (None, false) => format!("{}  Offline", icon::WIFI_OFF),
        };
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
            ui.label(caption(self.selection.title(&self.catalog, self.family()), &p));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let next = match self.step {
                    Step::Review => "Begin installation".to_owned(),
                    Step::Desktop => format!("Review  {}", icon::NEXT),
                    _ => format!("Next  {}", icon::NEXT),
                };
                let ready = self.step != Step::Review || Self::CAN_INSTALL;
                let button = ui.add_enabled_ui(ready, |ui| primary(ui, &p, &next, false)).inner;
                if button.clicked() {
                    actions.push(Action::Next);
                }
                if !ready {
                    ui.label(caption("Not in this preview build", &p));
                }
                let back = format!("{}  Back", icon::BACK);
                if self.step.back().is_some() && quiet(ui, &p, &back).clicked() {
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
            ui.label(caption(
                "Wired networks connect on their own. Without a connection you can still look around, but nothing can be downloaded.",
                &p,
            ));
        }
    }

    fn machine(&self, ui: &mut Ui) {
        let p = self.palette;
        let Some(hw) = self.hardware() else {
            ui.label(caption("Scanning…", &p));
            return;
        };
        let facts = machine_facts(hw);
        grid(ui, 3, &facts, |ui, (label, value, detail)| {
            info_card(ui, &p, label, value, detail);
        });
        let findings = machine_findings(hw);
        if !findings.is_empty() {
            ui.add_space(14.0);
            ui.label(eyebrow("What Borea will do", p.sub));
            ui.add_space(6.0);
            for finding in findings {
                let (mark, text) = match finding {
                    Finding::Recommend(text) => (icon::NEXT, text),
                    Finding::Unknown(text) => ("?", text),
                    Finding::Blocker(text) => (icon::CLOSE, text),
                };
                ui.label(RichText::new(format!("{mark}  {text}")).size(size::SMALL));
            }
        }
    }

    fn system(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        let catalog = &self.catalog;
        let s = &self.selection;
        let family = self.family();
        let choice = s.choice(family);

        section(ui, &p, "Family");
        grid(ui, 4, &Family::ALL, |ui, f| {
            let names: Vec<_> = catalog.family(*f).map(|d| d.name.as_str()).collect();
            let card = choice_card(
                ui,
                &p,
                ("family", *f),
                &format!("{f} family"),
                *f == family,
                true,
                |ui| {
                    ui.set_min_height(86.0);
                    ui.horizontal(|ui| {
                        glyph(ui, icon::family(*f), 22.0, p.accent);
                        ui.label(RichText::new(f.to_string()).size(size::HEADING));
                    });
                    ui.label(caption(names.join(" · "), &p));
                },
            );
            if card.clicked() {
                actions.push(Action::Family(*f));
            }
        });

        section(ui, &p, "Distribution");
        let distros: Vec<(usize, &Distro)> = catalog.family(family).enumerate().collect();
        grid(ui, 3, &distros, |ui, (i, d)| {
            let card =
                choice_card(ui, &p, ("distro", &d.id), &d.name, *i == choice.distro, true, |ui| {
                    ui.set_min_height(98.0);
                    ui.horizontal(|ui| {
                        glyph(ui, icon::distro(d.id.as_str()), size::HEADING, p.accent);
                        ui.label(RichText::new(&d.name).size(size::HEADING - 2.0));
                    });
                    ui.label(caption(&d.summary, &p));
                    ui.label(
                        RichText::new(capitalize(&d.channel.to_string()))
                            .size(size::TINY)
                            .color(p.accent),
                    );
                });
            if card.clicked() {
                actions.push(Action::Distro(family, *i));
            }
        });

        section(ui, &p, "Edition");
        let distro = s.distro(catalog, family);
        let editions: Vec<(usize, &Edition)> = distro.editions.iter().enumerate().collect();
        grid(ui, 4, &editions, |ui, (i, e)| {
            let card = choice_card(
                ui,
                &p,
                ("edition", &e.id),
                &e.name,
                *i == choice.edition,
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

        if !distro.variants.is_empty() {
            section(ui, &p, "Image variant");
            let variants: Vec<(usize, &String)> = distro.variants.iter().enumerate().collect();
            grid(ui, 4, &variants, |ui, (i, v)| {
                if choice_card(ui, &p, ("variant", *i), v, *i == choice.variant, true, |ui| {
                    ui.label(RichText::new(v.as_str()).size(size::BODY));
                })
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
            if choice_card(ui, &p, ("mode", *name), name, choice.mode == *mode, true, |ui| {
                ui.label(RichText::new(*name).size(size::HEADING - 2.0));
                ui.label(caption(*about, &p));
            })
            .clicked()
            {
                actions.push(Action::Mode(family, *mode));
            }
        });
        if choice.mode == DesktopMode::Bare {
            return;
        }

        section(ui, &p, &format!("{} Compositor", icon::ONE));
        let compositors: Vec<_> = catalog.desktops.compositors(&distro.id).collect();
        grid(ui, 3, &compositors, |ui, offer| {
            let c = offer.item;
            let on = choice.compositor.as_deref() == Some(c.id.as_str());
            let card =
                choice_card(ui, &p, ("compositor", &c.id), &c.name, on, offer.available(), |ui| {
                    ui.set_min_height(92.0);
                    ui.label(RichText::new(&c.name).size(size::HEADING - 2.0));
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

        section(ui, &p, &format!("{} Shell", icon::TWO));
        let compositor = choice.compositor.clone().unwrap_or_default();
        let compositor_name = s.compositor_name(catalog, family).unwrap_or_default().to_owned();
        let mut shells: Vec<Option<borea_catalog::Offer<'_, borea_catalog::Shell>>> =
            catalog.desktops.ranked(&distro.id, &compositor).map(Some).collect();
        shells.push(None);
        let suggested = catalog.desktops.suggested(&distro.id, &compositor).map(|s| s.id.clone());
        grid(ui, 2, &shells, |ui, offer| match offer {
            Some(offer) => {
                let shell = offer.item;
                let on = choice.shell == Some(ShellPick::Shell(shell.id.clone()));
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
                let on = choice.shell == Some(ShellPick::NoShell);
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
        ui.label(caption(s.arrival(catalog, family), &p));
    }

    fn review(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        let s = &self.selection;
        let family = self.family();
        if !Self::CAN_INSTALL {
            ui.set_max_width(660.0);
            notice(
                ui,
                &p,
                "This is a preview build: it cannot install yet. The review below is what Borea would install; nothing on this machine changes.",
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
                match self.secure_boot() {
                    Some(Readiness::Available { enrollment }) => {
                        let mut on = self.selection.secure_boot;
                        let options =
                            [(false, "Leave Secure Boot off"), (true, "Turn Secure Boot on")];
                        if segmented(ui, &p, &mut on, &options) {
                            actions.push(Action::SecureBoot(on));
                        }
                        ui.add_space(4.0);
                        if on {
                            for step in secure_boot::STEPS {
                                ui.label(
                                    RichText::new(format!("{}  {step}", icon::CHECK))
                                        .size(size::SMALL),
                                );
                            }
                            ui.add_space(4.0);
                            ui.label(caption(enrollment.describe(), &p));
                        } else {
                            ui.label(caption(
                                "The default. You can turn it on later from the installed system.",
                                &p,
                            ));
                        }
                    }
                    Some(Readiness::Unavailable(why)) => {
                        ui.label(caption(format!("Not available here: {why}."), &p));
                    }
                    None => {}
                }
                if let Some(warning) = secure_boot::conflict(self.selection.secure_boot, hw) {
                    ui.add_space(6.0);
                    let text = RichText::new(warning).size(size::SMALL);
                    tag(ui, p.wash, Stroke::new(1.0, p.accent), text);
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

fn fact_icon(label: &str) -> &'static str {
    match label {
        "Processor" => icon::CPU,
        "Graphics" => icon::GPU,
        "Memory" => icon::MEMORY,
        "Firmware" => icon::SHIELD,
        "Device" => icon::LAPTOP,
        "Display" => icon::MONITOR,
        "Disk" => icon::DISK,
        "Network" => icon::ETHERNET,
        _ => icon::CHIP,
    }
}

/// The machine as label, value, and detail, one card each.
fn machine_facts(hw: &HardwareSnapshot) -> Vec<(&'static str, String, String)> {
    let mut facts = Vec::new();
    match &hw.cpu {
        Fact::Known(cpu) => {
            let level = cpu.x86_level.map(|l| format!(" · {l}")).unwrap_or_default();
            facts.push((
                "Processor",
                cpu.model.clone().unwrap_or_else(|| "Unknown model".into()),
                format!("{} threads{level}", cpu.logical_cores),
            ));
        }
        Fact::Unknown { reason } => facts.push(("Processor", "Unknown".into(), reason.clone())),
    }
    for gpu in &hw.gpus {
        let driver = gpu.host_driver.clone().unwrap_or_else(|| "no driver in this session".into());
        facts.push(("Graphics", gpu.label(), driver));
    }
    let memory =
        hw.memory_bytes.known().map_or("Unknown".into(), |b| format!("{:.0} GB", *b as f64 / 1e9));
    facts.push(("Memory", memory, String::new()));
    let secure = match hw.secure_boot {
        Fact::Known(true) => "Secure Boot on",
        Fact::Known(false) => "Secure Boot off",
        Fact::Unknown { .. } => "Secure Boot unknown",
    };
    facts.push(("Firmware", hw.firmware.to_string(), secure.into()));
    let detail = match (&hw.device_class, hw.battery) {
        (Fact::Unknown { reason }, _) => capitalize(reason),
        (Fact::Known(_), true) => "battery".into(),
        (Fact::Known(_), false) => String::new(),
    };
    facts.push(("Device", device_name(hw), detail));
    for display in &hw.displays {
        let place = if display.internal { "Built-in" } else { "External" };
        facts.push((
            "Display",
            format!("{place} · {}", display.connector),
            display.mode.clone().unwrap_or_default(),
        ));
    }
    for disk in &hw.disks {
        let usage = match &disk.usage {
            DiskUsage::Free => "Available to choose later".to_owned(),
            other => capitalize(&other.to_string()),
        };
        facts.push((
            "Disk",
            format!("{} · {:.0} GB", disk.name, disk.size_bytes as f64 / 1e9),
            usage,
        ));
    }
    for nic in &hw.network {
        let kind = if nic.wireless { "Wi-Fi" } else { "Wired" };
        facts.push((
            "Network",
            format!("{kind} · {}", nic.interface),
            nic.host_driver.clone().unwrap_or_default(),
        ));
    }
    facts
}
