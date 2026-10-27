//! The Windpad window.
//!
//! Each frame renders from `&self` and records what the user did as [`Action`]s, which
//! are applied once at the end. Every change to state happens in [`Windpad::apply`].

use std::sync::mpsc::{self, Receiver};
use std::thread;

use borea_hardware::HardwareSnapshot;
use borea_live_catalog::Catalog;
use borea_live_core::{Fact, Family, FirmwareMode};
use eframe::egui::{
    self, Align, CentralPanel, ComboBox, CornerRadius, Frame, Id, Layout, Margin, RichText,
    ScrollArea, Stroke, Ui, UiBuilder, text::LayoutJob,
};

use crate::firmware::{Choice, FirmwareLink};
use crate::icon;
use crate::model::{
    DesktopMode, Selection, ShellPick, capitalize, compose_teaser, device_name, family_line,
    gigabytes, hardware_facts,
};
use crate::theme::{self, DARK, LIGHT, Palette, RADIUS, size};
use crate::widgets::{
    self, badge, caption, choice_card, dialog, dialog_head, eyebrow, glyph, notice, primary, quiet,
    rows, segmented,
};
use crate::wifi::{Command, WifiForm, WifiLink};
use crate::wind;

/// The content column: wide enough for four family cards side by side.
const MAX_WIDTH: f32 = 1600.0;
/// The narrowest a family card gets before the grid drops to fewer columns.
const MIN_CARD: f32 = 380.0;
/// Space between family cards.
const GAP: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Compositor,
    Shell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Hardware,
    Wifi,
    Compose(Family, Step),
    Continue(Family),
    /// The confirmation before restarting into UEFI firmware setup.
    Firmware,
}

/// Everything a user can do, recorded during rendering.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Select(Family),
    Distro(Family, usize),
    Edition(Family, usize),
    Variant(Family, usize),
    Mode(Family, DesktopMode),
    Compositor(Family, String),
    Shell(Family, ShellPick),
    Extra(Family, String, bool),
    /// Switch the card to Minimal (on another distribution if need be) to compose.
    Compose(Family, (usize, usize)),
    UseDesktop(Family),
    SkipDesktop(Family),
    Open(Dialog),
    Close,
    ToggleTheme,
    ToggleMotion,
    RestartFirmware,
}

enum Scan {
    Pending(Receiver<HardwareSnapshot>),
    Done(Box<HardwareSnapshot>),
}

pub struct Windpad {
    catalog: Catalog,
    selection: Selection,
    palette: Palette,
    still: bool,
    scan: Scan,
    wifi: WifiLink,
    form: WifiForm,
    dialog: Option<Dialog>,
    firmware: FirmwareLink,
}

impl Windpad {
    /// The real window: scans hardware and talks to NetworkManager in the background.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = cc.egui_ctx.clone();
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        thread::Builder::new()
            .name("windpad-scan".into())
            .spawn(move || {
                let _ = tx.send(borea_hardware::scan());
                repaint.request_repaint();
            })
            .expect("spawn hardware scan");
        let dark = ctx.system_theme() == Some(egui::Theme::Dark);
        let catalog = Catalog::bundled().expect("bundled catalog is validated by tests");
        let mut app =
            Self::assemble(&ctx, catalog, Scan::Pending(rx), WifiLink::spawn(ctx.clone()), dark);
        app.firmware = FirmwareLink::system(ctx);
        app
    }

    /// A still window with fixed hardware and no network backend, for tests and
    /// previews. Holding the wind still lets each frame settle.
    pub fn with_hardware(ctx: &egui::Context, hardware: HardwareSnapshot, dark: bool) -> Self {
        let catalog = Catalog::bundled().expect("bundled catalog");
        let wifi = WifiLink::offline("NetworkManager is not connected in this preview");
        let mut app = Self::assemble(ctx, catalog, Scan::Done(Box::new(hardware)), wifi, dark);
        app.still = true;
        app
    }

    fn assemble(
        ctx: &egui::Context,
        catalog: Catalog,
        scan: Scan,
        wifi: WifiLink,
        dark: bool,
    ) -> Self {
        theme::install_fonts(ctx);
        let palette = if dark { DARK } else { LIGHT };
        theme::apply(ctx, &palette);
        let mut selection = Selection::default();
        for family in Family::ALL {
            selection.ensure_defaults(&catalog, family);
        }
        Windpad {
            catalog,
            selection,
            palette,
            still: false,
            scan,
            wifi,
            form: WifiForm::default(),
            dialog: None,
            firmware: FirmwareLink::preview(),
        }
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn dialog(&self) -> Option<Dialog> {
        self.dialog
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
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
    }

    fn uefi(&self) -> bool {
        self.hardware()
            .is_some_and(|hw| matches!(hw.firmware, Fact::Known(FirmwareMode::Uefi { .. })))
    }

    pub fn apply(&mut self, ctx: &egui::Context, action: Action) {
        let catalog = &self.catalog;
        let s = &mut self.selection;
        match action {
            Action::Select(f) => s.select(f),
            Action::Distro(f, i) => s.set_distro(f, i),
            Action::Edition(f, i) => s.set_edition(f, i),
            Action::Variant(f, i) => s.set_variant(f, i),
            Action::Mode(f, mode) => s.set_mode(f, mode),
            Action::Compositor(f, id) => {
                s.set_compositor(catalog, f, &id);
                self.dialog = Some(Dialog::Compose(f, Step::Shell));
            }
            Action::Shell(f, pick) => s.set_shell(f, pick),
            Action::Extra(f, id, on) => s.set_extra(f, &id, on),
            Action::Compose(f, target) => s.compose_on(f, target),
            Action::UseDesktop(f) => {
                s.confirm(f);
                self.dialog = None;
            }
            Action::SkipDesktop(f) => {
                s.set_mode(f, DesktopMode::Bare);
                self.dialog = None;
            }
            Action::Open(dialog) => {
                if let Dialog::Compose(f, _) | Dialog::Continue(f) = dialog {
                    s.select(f);
                }
                if dialog == Dialog::Wifi {
                    self.wifi.send(Command::Refresh);
                }
                self.dialog = Some(dialog);
            }
            // A restart already on its way cannot be cancelled.
            Action::Close if self.firmware.restarting => {}
            Action::Close => {
                self.dialog = None;
                self.form.forget_passphrase();
            }
            Action::RestartFirmware => {
                if self.dialog == Some(Dialog::Firmware) && self.uefi() {
                    self.firmware.reboot(ctx.clone());
                }
            }
            Action::ToggleTheme => {
                self.palette = if self.palette.dark { LIGHT } else { DARK };
                theme::apply(ctx, &self.palette);
            }
            Action::ToggleMotion => self.still = !self.still,
        }
        if let Some(Dialog::Compose(f, _)) = self.dialog {
            self.selection.ensure_defaults(&self.catalog, f);
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
        let focused = ui.input(|i| i.focused);
        let moving = !self.still && focused && !matches!(self.dialog, Some(Dialog::Continue(_)));

        CentralPanel::no_frame().frame(Frame::new().fill(p.bg)).show(ui, |ui| {
            concept_banner(ui, &p);
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                column(ui, |ui| {
                    self.header(ui, &mut actions);
                    self.hero(ui, moving);
                    self.hardware_strip(ui);
                    section_line(ui, &p);
                    self.cards(ui, &mut actions);
                    self.footer(ui, &mut actions);
                });
            });
        });

        match self.dialog {
            Some(Dialog::Compose(f, step)) => self.compose_dialog(ui.ctx(), f, step, &mut actions),
            Some(Dialog::Continue(f)) => self.continue_dialog(ui.ctx(), f, &mut actions),
            Some(Dialog::Hardware) => self.hardware_dialog(ui.ctx(), &mut actions),
            Some(Dialog::Wifi) => self.wifi_dialog(ui.ctx(), &mut actions),
            Some(Dialog::Firmware) => match self.firmware.confirmation(ui.ctx(), &p) {
                Some(Choice::Cancel) => actions.push(Action::Close),
                Some(Choice::Restart) => actions.push(Action::RestartFirmware),
                None => {}
            },
            None => {}
        }

        if moving {
            ui.ctx().request_repaint_after(wind::FRAME);
        }
        actions
    }

    // ── Page ────────────────────────────────────────────────────────────────

    fn header(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::WIND).size(20.0).color(p.accent));
            ui.label(
                RichText::new("BOREA")
                    .family(theme::display())
                    .size(20.0)
                    .extra_letter_spacing(3.5)
                    .color(p.ink),
            );
            ui.add_space(6.0);
            ui.label(eyebrow("Windpad", p.sub));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let theme = if p.dark { "Light appearance" } else { "Dark appearance" };
                let toggle = nav_button(ui, &p, icon::THEME, theme).on_hover_text(theme);
                if toggle.clicked() {
                    actions.push(Action::ToggleTheme);
                }
                let symbol = if self.wifi.connected() { icon::WIFI } else { icon::WIFI_OFF };
                let wifi = format!("{symbol}  {}", self.wifi.connection_label());
                if nav_button(ui, &p, &wifi, "Wi-Fi").clicked() {
                    actions.push(Action::Open(Dialog::Wifi));
                }
                let hardware = format!("{}  Hardware", icon::CHIP);
                if nav_button(ui, &p, &hardware, "Hardware").clicked() {
                    actions.push(Action::Open(Dialog::Hardware));
                }
                let (enabled, hint) = self.firmware.control(self.uefi());
                let firmware = ui
                    .add_enabled_ui(enabled, |ui| {
                        nav_button(ui, &p, &FirmwareLink::label(), "UEFI setup")
                    })
                    .inner
                    .on_disabled_hover_text(hint)
                    .on_hover_text(hint);
                if firmware.clicked() {
                    actions.push(Action::Open(Dialog::Firmware));
                }
            });
        });
        ui.add_space(18.0);
        hairline(ui, &p);
    }

    fn hero(&self, ui: &mut Ui, moving: bool) {
        let p = self.palette;
        let height = 290.0;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
        let time = moving.then(|| ui.input(|i| i.time));
        wind::paint(&ui.painter_at(rect.expand2(egui::vec2(0.0, 20.0))), rect, p.accent, time);

        ui.scope_builder(
            UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Center)),
            |ui| {
                ui.add_space(58.0);
                ui.label(eyebrow("A little direction. Endless possibility.", p.accent));
                ui.add_space(12.0);
                let mut job = LayoutJob::default();
                let serif = |color| egui::TextFormat {
                    font_id: egui::FontId::new(size::HERO, theme::display()),
                    color,
                    ..Default::default()
                };
                job.append("Go wherever\nthe ", 0.0, serif(p.ink));
                job.append("wind", 0.0, serif(p.accent));
                job.append(" takes you.", 0.0, serif(p.ink));
                job.halign = Align::Center;
                ui.label(job);
                ui.add_space(14.0);
                ui.label(caption("Your hardware. Your distribution. A fresh beginning.", &p));
                ui.label(caption("Choose your next Linux system. We'll prepare the way.", &p));
            },
        );
    }

    fn hardware_strip(&self, ui: &mut Ui) {
        let p = self.palette;
        let facts: [(&str, String); 4] = match self.hardware() {
            None => {
                ["Processor", "Graphics", "Memory", "Platform"].map(|k| (k, "Scanning…".into()))
            }
            Some(hw) => [
                (
                    "Processor",
                    hw.cpu
                        .known()
                        .and_then(|c| c.model.clone())
                        .unwrap_or_else(|| "Unknown".into()),
                ),
                (
                    "Graphics",
                    match hw.gpus.as_slice() {
                        [] => "None detected".into(),
                        gpus => gpus
                            .iter()
                            .map(|g| g.vendor.to_string())
                            .collect::<Vec<_>>()
                            .join(" + "),
                    },
                ),
                ("Memory", hw.memory_bytes.known().map_or("Unknown".into(), |b| gigabytes(*b))),
                ("Platform", format!("{} · {}", hw.firmware, device_name(hw))),
            ],
        };
        hairline(ui, &p);
        ui.add_space(14.0);
        let icons = [icon::CPU, icon::GPU, icon::MEMORY, icon::CHIP];
        ui.columns(4, |cols| {
            for ((col, (label, value)), mark) in cols.iter_mut().zip(facts).zip(icons) {
                col.horizontal(|ui| {
                    glyph(ui, mark, size::SMALL, p.accent);
                    ui.label(eyebrow(label, p.sub));
                });
                col.add(
                    egui::Label::new(RichText::new(value).size(size::SMALL).strong()).truncate(),
                );
            }
        });
        ui.add_space(14.0);
        hairline(ui, &p);
        ui.add_space(30.0);
    }

    /// The family cards in as many columns as the width holds (four, two, or one),
    /// each row as tall as its tallest card.
    fn cards(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let order = [Family::Arch, Family::Fedora, Family::Debian, Family::Opensuse];
        let fit = ((ui.available_width() + GAP) / (MIN_CARD + GAP)).floor() as usize;
        // Four, two, or one: three would leave one card alone on a second row.
        let cols = match fit {
            4.. => 4,
            2 | 3 => 2,
            _ => 1,
        };
        for (r, row) in order.chunks(cols).enumerate() {
            // Each card's natural height is measured as it draws, so the row matches
            // its tallest card from the next frame on, growing and shrinking with it.
            let id = Id::unique(("windpad-card-row", cols, r));
            let target: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
            let mut tallest = 0.0_f32;
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                ui.columns(cols, |columns| {
                    for (column, family) in columns.iter_mut().zip(row) {
                        tallest = tallest.max(self.card(column, *family, target, actions));
                    }
                });
            });
            if (tallest - target).abs() > 0.5 {
                ui.ctx().data_mut(|d| d.insert_temp(id, tallest));
                ui.ctx().request_repaint();
            }
            ui.add_space(GAP);
        }
    }

    /// One family card, its body at least `height` tall so the feet of a row line up.
    /// Returns the body's natural height.
    fn card(&self, ui: &mut Ui, family: Family, height: f32, actions: &mut Vec<Action>) -> f32 {
        let p = self.palette;
        let selected = self.selection.selected == family;
        let stroke = if selected { Stroke::new(1.6, p.accent) } else { Stroke::new(1.0, p.line) };

        Frame::new()
            .fill(p.paper)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(RADIUS + 1))
            .inner_margin(Margin::same(22))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // `columns` justifies its children; cards read better left-aligned.
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    let natural = ui
                        .scope(|ui| self.card_contents(ui, family, actions))
                        .response
                        .rect
                        .height();
                    ui.add_space((height - natural).max(0.0));
                    self.card_foot(ui, family, actions);
                    natural
                })
                .inner
            })
            .inner
    }

    fn card_contents(&self, ui: &mut Ui, family: Family, actions: &mut Vec<Action>) {
        let p = self.palette;
        let catalog = &self.catalog;
        let s = &self.selection;
        let choice = s.choice(family);
        let distro = s.distro(catalog, family);
        let selected = s.selected == family;

        ui.horizontal(|ui| {
            glyph(ui, icon::family(family), 26.0, p.accent);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(family.to_string()).family(theme::display()).size(size::HEADING),
                );
                ui.label(caption(family_line(family), &p));
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                if select_mark(ui, &p, family, selected).clicked() {
                    actions.push(Action::Select(family));
                }
            });
        });
        ui.add_space(10.0);

        let distros: Vec<_> = catalog.family(family).collect();
        let names = |list: &[String], i: usize| list[i].clone();
        let editions: Vec<String> = distro.editions.iter().map(|e| e.name.clone()).collect();
        let distro_names: Vec<String> = distros
            .iter()
            .map(|d| format!("{}  {}", icon::distro(d.id.as_str()), d.name))
            .collect();
        actions.extend(
            field(ui, &p, "Distribution", (family, "distro"), choice.distro, &distro_names, names)
                .map(|i| Action::Distro(family, i)),
        );
        actions.extend(
            field(ui, &p, "Edition", (family, "edition"), choice.edition, &editions, names)
                .map(|i| Action::Edition(family, i)),
        );
        if !distro.variants.is_empty() {
            actions.extend(
                field(
                    ui,
                    &p,
                    "Image variant",
                    (family, "variant"),
                    choice.variant,
                    &distro.variants,
                    names,
                )
                .map(|i| Action::Variant(family, i)),
            );
        }

        self.desktop_block(ui, family, actions);
        ui.add_space(4.0);
        ui.label(caption(&distro.summary, &p));
        if distro.source.is_none() {
            ui.label(caption("Upstream source pending verification.", &p));
        }
    }

    /// The card's last lines, level across a row: channel and status, then the
    /// preview once the path is complete.
    fn card_foot(&self, ui: &mut Ui, family: Family, actions: &mut Vec<Action>) {
        let p = self.palette;
        let catalog = &self.catalog;
        let s = &self.selection;
        let distro = s.distro(catalog, family);
        let selected = s.selected == family;
        ui.add_space(8.0);
        hairline(ui, &p);
        ui.horizontal(|ui| {
            ui.label(caption(capitalize(&distro.channel.to_string()), &p));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let status = capitalize(&distro.status.to_string());
                ui.label(caption(format!("{status} integration"), &p));
            });
        });

        if selected && s.path_complete(catalog, family) {
            self.preview(ui, family, actions);
        }
    }

    /// Your desktop, at the foot of every card: the compose path on Minimal, what
    /// the edition brings otherwise, and the Gaming checkbox.
    fn desktop_block(&self, ui: &mut Ui, family: Family, actions: &mut Vec<Action>) {
        let p = self.palette;
        let catalog = &self.catalog;
        let s = &self.selection;
        let distro = s.distro(catalog, family);
        let edition = s.edition(catalog, family);
        ui.add_space(4.0);
        Frame::new()
            .fill(p.bg)
            .stroke(Stroke::new(1.0, p.line))
            .corner_radius(CornerRadius::same(RADIUS))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Your desktop").size(size::SMALL).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let note = if edition.composable {
                            format!("{} ships without one", edition.name)
                        } else {
                            "Included".to_owned()
                        };
                        ui.label(caption(note, &p));
                    });
                });
                if edition.composable {
                    self.compose(ui, family, actions);
                } else {
                    ui.label(caption(
                        format!("{}, as {} ships it.", edition.name, distro.name),
                        &p,
                    ));
                    if let Some(target) = s.compose_target(catalog, family) {
                        let on = catalog
                            .family(family)
                            .nth(target.0)
                            .expect("target indexes the family");
                        let title = format!("Or compose your own desktop on {} Minimal", on.name);
                        ui.add_space(4.0);
                        let teaser = compose_teaser(catalog, on);
                        if widgets::compose_offer(ui, &p, (family, "compose"), &title, &teaser)
                            .clicked()
                        {
                            actions.push(Action::Compose(family, target));
                        }
                    }
                }
                ui.add_space(8.0);
                hairline(ui, &p);
                ui.add_space(6.0);
                ui.label(eyebrow("Extras", p.sub));
                let ticked = |id: &str| s.extra(catalog, family, id);
                if let Some((id, on)) = widgets::extras(ui, &p, catalog, distro, ticked) {
                    actions.push(Action::Extra(family, id, on));
                }
            });
    }

    /// Compose a desktop or go without, then the compositor and shell steps.
    fn compose(&self, ui: &mut Ui, family: Family, actions: &mut Vec<Action>) {
        let p = self.palette;
        let catalog = &self.catalog;
        let s = &self.selection;
        let mut mode = s.choice(family).mode;
        if segmented(
            ui,
            &p,
            &mut mode,
            &[(DesktopMode::Compose, "Compose a desktop"), (DesktopMode::Bare, "No desktop")],
        ) {
            actions.push(Action::Mode(family, mode));
        }
        if mode == DesktopMode::Bare {
            ui.label(caption(s.bare_summary(catalog, family), &p));
            return;
        }
        let compositor = s.compositor_name(catalog, family).unwrap_or("Choose").to_owned();
        let shell = s.shell_name(catalog, family).to_owned();
        let distro = &s.distro(catalog, family).name;
        ui.columns(2, |cols| {
            for (col, (step, n, label, value)) in cols.iter_mut().zip([
                (Step::Compositor, icon::ONE, "Compositor", compositor),
                (Step::Shell, icon::TWO, "Shell", shell),
            ]) {
                let a11y =
                    format!("Choose {} for {distro}, currently {value}", label.to_lowercase());
                let response = choice_card(col, &p, (family, label), &a11y, false, true, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(n).size(size::HEADING).color(p.accent));
                        ui.vertical(|ui| {
                            ui.label(RichText::new(label).size(size::TINY).color(p.sub));
                            ui.add(
                                egui::Label::new(RichText::new(&value).size(size::SMALL).strong())
                                    .truncate(),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(caption(icon::CHEVRON, &p))
                        });
                    });
                });
                if response.clicked() {
                    actions.push(Action::Open(Dialog::Compose(family, step)));
                }
            }
        });
    }

    fn preview(&self, ui: &mut Ui, family: Family, actions: &mut Vec<Action>) {
        let p = self.palette;
        let s = &self.selection;
        ui.add_space(14.0);
        Frame::new()
            .fill(p.wash)
            .corner_radius(CornerRadius::same(RADIUS))
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(eyebrow("Your selection", p.accent));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(caption("Concept · no disk changes", &p));
                    });
                });
                ui.label(RichText::new(s.title(&self.catalog, family)).size(size::BODY + 1.0));
                ui.add_space(6.0);
                let all = s.rows(&self.catalog, family);
                rows(ui, &p, &all[1..]);
                ui.add_space(10.0);
                if primary(ui, &p, &format!("Continue with this selection  {}", icon::NEXT), true)
                    .clicked()
                {
                    actions.push(Action::Open(Dialog::Continue(family)));
                }
            });
    }

    fn footer(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let p = self.palette;
        ui.add_space(16.0);
        hairline(ui, &p);
        ui.horizontal(|ui| {
            ui.label(caption("Made to move you forward. Gently.", &p));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = if self.still { "Wind paused" } else { "Pause wind" };
                if quiet(ui, &p, label).clicked() {
                    actions.push(Action::ToggleMotion);
                }
            });
        });
        ui.add_space(20.0);
    }

    // ── Dialogs ─────────────────────────────────────────────────────────────

    fn compose_dialog(
        &self,
        ctx: &egui::Context,
        family: Family,
        step: Step,
        actions: &mut Vec<Action>,
    ) {
        let p = self.palette;
        let catalog = &self.catalog;
        let s = &self.selection;
        let distro = s.distro(catalog, family);
        let edition = s.edition(catalog, family);
        let choice = s.choice(family);

        let modal = dialog(ctx, "compose", &p, 720.0, |ui| {
            if dialog_head(ui, &p, "Compose your desktop.") {
                actions.push(Action::Close);
            }
            ui.label(caption(format!("{} · {}", distro.name, edition.name), &p));
            ui.add_space(10.0);
            let mut tab = step;
            segmented(
                ui,
                &p,
                &mut tab,
                &[
                    (Step::Compositor, &format!("{}  Compositor", icon::ONE)),
                    (Step::Shell, &format!("{}  Shell", icon::TWO)),
                ],
            );
            if tab != step {
                actions.push(Action::Open(Dialog::Compose(family, tab)));
            }
            ui.add_space(12.0);

            match step {
                Step::Compositor => {
                    let offers: Vec<_> = catalog.desktops.compositors(&distro.id).collect();
                    ui.columns(offers.len(), |cols| {
                        for (col, offer) in cols.iter_mut().zip(&offers) {
                            let c = offer.item;
                            let on = choice.compositor.as_deref() == Some(c.id.as_str());
                            let response = choice_card(
                                col,
                                &p,
                                ("compositor", &c.id),
                                &c.name,
                                on,
                                offer.available(),
                                |ui| {
                                    ui.set_min_height(104.0);
                                    ui.horizontal(|ui| {
                                        glyph(ui, icon::compositor(&c.id), size::HEADING, p.accent);
                                        ui.label(RichText::new(&c.name).size(size::HEADING - 2.0));
                                    });
                                    ui.label(caption(&c.style, &p));
                                    match offer.source {
                                        Some(source) => badge(ui, &p, source),
                                        None => {
                                            ui.label(caption(
                                                format!("Not packaged for {}", distro.name),
                                                &p,
                                            ));
                                        }
                                    }
                                },
                            );
                            if response.clicked() {
                                actions.push(Action::Compositor(family, c.id.clone()));
                            }
                        }
                    });
                }
                Step::Shell => {
                    let compositor = choice.compositor.clone().unwrap_or_default();
                    let compositor_name = s.compositor_name(catalog, family).unwrap_or_default();
                    ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                        for (i, offer) in catalog.desktops.ranked(&distro.id, &compositor).enumerate() {
                            let shell = offer.item;
                            let on = choice.shell == Some(ShellPick::Shell(shell.id.clone()));
                            let response = choice_card(ui, &p, ("shell", &shell.id), &shell.name, on, true, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&shell.name).size(size::BODY + 1.0));
                                    ui.label(caption(&shell.by, &p));
                                    if i == 0 {
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            let why = if shell.made_for(&distro.id) {
                                                format!("{} Made for {}", icon::STAR, distro.name)
                                            } else {
                                                format!("{} Suggested", icon::STAR)
                                            };
                                            ui.label(RichText::new(why).size(size::TINY).color(p.accent));
                                        });
                                    }
                                });
                                ui.label(caption(&shell.summary, &p));
                                ui.horizontal(|ui| {
                                    if let Some(source) = offer.source {
                                        badge(ui, &p, source);
                                    }
                                    ui.label(caption(format!("{} · {}", shell.framework, shell.license), &p));
                                });
                            });
                            if response.clicked() {
                                actions.push(Action::Shell(family, ShellPick::Shell(shell.id.clone())));
                            }
                            ui.add_space(2.0);
                        }
                        let on = choice.shell == Some(ShellPick::NoShell);
                        let response = choice_card(ui, &p, "no-shell", "No shell", on, true, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("No shell").size(size::BODY + 1.0));
                                ui.label(caption(format!("just {compositor_name}"), &p));
                            });
                            ui.label(caption(
                                format!("{compositor_name} on its own, with its default config. Bring your own bar and launcher later."),
                                &p,
                            ));
                        });
                        if response.clicked() {
                            actions.push(Action::Shell(family, ShellPick::NoShell));
                        }
                    });
                }
            }

            ui.add_space(12.0);
            hairline(ui, &p);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width() - 300.0);
                    let summary = format!(
                        "{} · {}",
                        s.compositor_name(catalog, family).unwrap_or_default(),
                        s.shell_name(catalog, family)
                    );
                    ui.label(RichText::new(summary).size(size::BODY).strong());
                    ui.label(caption(s.arrival(catalog, family), &p));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let next = if step == Step::Compositor {
                        format!("Choose a shell  {}", icon::NEXT)
                    } else {
                        "Use this desktop".to_owned()
                    };
                    if primary(ui, &p, &next, false).clicked() {
                        actions.push(match step {
                            Step::Compositor => Action::Open(Dialog::Compose(family, Step::Shell)),
                            Step::Shell => Action::UseDesktop(family),
                        });
                    }
                    if quiet(ui, &p, "Skip · no desktop").clicked() {
                        actions.push(Action::SkipDesktop(family));
                    }
                });
            });
        });
        if modal.should_close() {
            actions.push(Action::Close);
        }
    }

    fn continue_dialog(&self, ctx: &egui::Context, family: Family, actions: &mut Vec<Action>) {
        let p = self.palette;
        let modal = dialog(ctx, "continue", &p, 480.0, |ui| {
            if dialog_head(ui, &p, "Your next direction.") {
                actions.push(Action::Close);
            }
            notice(
                ui,
                &p,
                "Interface preview only. No release has been resolved and no adapter is installed. This cannot download or install a system yet.",
            );
            ui.add_space(8.0);
            rows(ui, &p, &self.selection.rows(&self.catalog, family));
            ui.add_space(8.0);
            ui.label(caption("Borea continues to storage, account settings, and a complete installation review. Latest upstream releases and hardware eligibility are verified before an install can be approved.", &p));
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if primary(ui, &p, "Keep exploring", false).clicked() {
                    actions.push(Action::Close);
                }
            });
        });
        if modal.should_close() {
            actions.push(Action::Close);
        }
    }

    fn hardware_dialog(&self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let p = self.palette;
        let modal = dialog(ctx, "hardware", &p, 520.0, |ui| {
            if dialog_head(ui, &p, "A sense of your system.") {
                actions.push(Action::Close);
            }
            let Some(hw) = self.hardware() else {
                ui.label(caption("Scanning…", &p));
                return;
            };
            notice(
                ui,
                &p,
                "Read on this machine, read-only. Unknown stays unknown, and disks need your explicit choice.",
            );
            ui.add_space(8.0);
            ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                rows(ui, &p, &hardware_rows(hw));
                widgets::findings(ui, &p, hw);
            });
        });
        if modal.should_close() {
            actions.push(Action::Close);
        }
    }

    fn wifi_dialog(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let p = self.palette;
        let Windpad { wifi, form, .. } = self;
        let modal = dialog(ctx, "wifi", &p, 500.0, |ui| {
            if dialog_head(ui, &p, "Catch a connection.") {
                actions.push(Action::Close);
            }
            if let Some(command) = crate::wifi::panel(ui, &p, wifi, form) {
                wifi.send(command);
            }
        });
        if modal.should_close() {
            actions.push(Action::Close);
        }
    }
}

impl eframe::App for Windpad {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for action in self.frame(ui) {
            self.apply(&ctx, action);
        }
    }
}

// ── Pieces ──────────────────────────────────────────────────────────────────

/// Center content in a column no wider than [`MAX_WIDTH`], with side gutters.
fn column(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let gutter = if ui.available_width() < 700.0 { 16.0 } else { 48.0 };
    let width = (ui.available_width() - 2.0 * gutter).min(MAX_WIDTH);
    let margin = (ui.available_width() - width) / 2.0;
    ui.horizontal_top(|ui| {
        ui.add_space(margin);
        ui.vertical(|ui| {
            ui.set_width(width);
            add(ui);
        });
    });
}

fn concept_banner(ui: &mut Ui, p: &Palette) {
    Frame::new().fill(p.wash).inner_margin(Margin::symmetric(16, 8)).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.label(eyebrow("Windpad · independent of any distribution ISO · installation unavailable in this build", p.sub));
        });
    });
}

fn section_line(ui: &mut Ui, p: &Palette) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Find your next direction").size(size::BODY).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(caption("Latest channels · versions resolved before install", p));
        });
    });
    ui.add_space(10.0);
}

fn hairline(ui: &mut Ui, p: &Palette) {
    let y = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), y, Stroke::new(1.0, p.line));
    ui.add_space(1.0);
}

/// A frameless header button, announced as `name`.
fn nav_button(ui: &mut Ui, p: &Palette, text: &str, name: &str) -> egui::Response {
    let button = egui::Button::new(RichText::new(text).size(size::SMALL).color(p.ink)).frame(false);
    let response = ui.add(button);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    response
}

/// A labelled dropdown over `options`. Returns the newly chosen index, if it changed.
fn field(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    id: impl egui::AsIdSalt,
    current: usize,
    options: &[String],
    name: impl Fn(&[String], usize) -> String,
) -> Option<usize> {
    let heading = ui.label(RichText::new(label).size(size::TINY).color(p.sub));
    let mut index = current;
    ComboBox::from_id_salt(id)
        .width(ui.available_width())
        .selected_text(name(options, current))
        .show_index(ui, &mut index, options.len(), |i| name(options, i))
        .labelled_by(heading.id);
    ui.add_space(4.0);
    (index != current).then_some(index)
}

/// The round select mark on each family card: ↗ to choose it, ✓ once chosen.
fn select_mark(ui: &mut Ui, p: &Palette, family: Family, selected: bool) -> egui::Response {
    let (mark, ink, fill) =
        if selected { (icon::CHECK, p.paper, p.accent) } else { (icon::OPEN, p.ink, p.paper) };
    let button = egui::Button::new(RichText::new(mark).color(ink))
        .fill(fill)
        .corner_radius(CornerRadius::same(14))
        .min_size(egui::vec2(28.0, 28.0));
    let response = ui.add(button);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Button,
            true,
            selected,
            format!("Select {family} family"),
        )
    });
    response
}

/// The Hardware drawer's rows: the shared machine facts, then the storage target.
fn hardware_rows(hw: &HardwareSnapshot) -> Vec<(&'static str, String)> {
    let mut rows: Vec<_> = hardware_facts(hw)
        .into_iter()
        .map(|(label, value, detail)| {
            (label, if detail.is_empty() { value } else { format!("{value} · {detail}") })
        })
        .collect();
    rows.push(("Storage target", "None selected".into()));
    rows
}
