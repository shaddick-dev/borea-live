//! Small building blocks Windpad and Borea Live share: cards you can choose, badges,
//! buttons, dialogs. Anything both windows draw comes from here, so they read as one design.

use borea_live_catalog::{Catalog, Distro, Source};
use eframe::egui::{
    self, Align, Button, CornerRadius, Frame, Layout, Margin, Response, RichText, Role, Sense,
    Stroke, Ui, WidgetInfo,
};

use crate::icon;
use crate::theme::{Palette, RADIUS, size};

/// A whole card that acts as one button: a compositor tile, a shell, a network.
/// Contents are laid out freely; the card announces itself with `label`.
///
/// Only the visible part of the card is clickable. Inside a scroll area, a card
/// scrolled out of view must not catch clicks meant for what is drawn there.
pub fn choice_card(
    ui: &mut Ui,
    p: &Palette,
    id: impl egui::AsIdSalt,
    label: &str,
    selected: bool,
    enabled: bool,
    add_contents: impl FnOnce(&mut Ui),
) -> Response {
    let id = ui.scope_id().with(id);
    let card = Frame::new()
        .fill(p.paper)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !enabled {
                ui.disable();
            }
            // Left-aligned, never justified, even inside `columns`.
            ui.with_layout(Layout::top_down(Align::Min), add_contents);
        });
    let rect = card.response.rect;
    // Clip to what is on screen. A card scrolled fully out of view gets an empty,
    // hover-only area: an inverted rectangle would span the space between the card
    // and the viewport and swallow clicks meant for whatever is drawn there.
    let visible = rect.intersect(ui.clip_rect());
    let (area, sense) = match (visible.is_positive(), enabled) {
        (true, true) => (visible, Sense::click()),
        (true, false) => (visible, Sense::hover()),
        (false, _) => (egui::Rect::from_min_size(rect.min, egui::Vec2::ZERO), Sense::hover()),
    };
    let response = ui.interact(area, id, sense);
    let highlight = match (selected, enabled && response.hovered()) {
        (true, _) => Some(Stroke::new(1.6, p.accent)),
        (false, true) => Some(Stroke::new(1.0, p.accent)),
        (false, false) => None,
    };
    if let Some(stroke) = highlight {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(RADIUS),
            stroke,
            egui::StrokeKind::Inside,
        );
    }
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, label));
    response
}

/// How a compositor or shell arrives, as a small label.
pub fn badge(ui: &mut Ui, p: &Palette, source: &Source) {
    let (fill, stroke, text) = match source {
        Source::Official { .. } => (p.wash, Stroke::NONE, p.accent),
        Source::ThirdParty { .. } | Source::Aur { .. } => {
            (egui::Color32::TRANSPARENT, Stroke::new(1.0, p.line), p.ink)
        }
        Source::FirstLogin { .. } => (egui::Color32::TRANSPARENT, Stroke::new(1.0, p.line), p.sub),
    };
    tag(ui, fill, stroke, RichText::new(source.badge()).size(size::TINY).color(text));
}

pub fn tag(ui: &mut Ui, fill: egui::Color32, stroke: Stroke, text: RichText) {
    Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(7, 3))
        .show(ui, |ui| ui.label(text));
}

/// The extras at the foot of Your desktop: a checkbox per optional feature the
/// distribution offers, each saying what it adds and any repository that needs,
/// then what its editions have built in. Returns the extra ticked or unticked.
pub fn extras(
    ui: &mut Ui,
    p: &Palette,
    catalog: &Catalog,
    distro: &Distro,
    ticked: impl Fn(&str) -> bool,
) -> Option<(String, bool)> {
    let mut change = None;
    let mut built_in = Vec::new();
    for offered in catalog.extras.on(&distro.id) {
        let extra = offered.extra;
        if offered.provision.built_in {
            built_in.push(extra.name.as_str());
            continue;
        }
        let mut on = ticked(&extra.id);
        let text = RichText::new(&extra.name).color(p.ink).size(size::SMALL + 0.5);
        let response = ui.checkbox(&mut on, text).on_hover_text(offered.describe());
        // Every card has the same extras, so each says whose it is.
        let label = format!("{} for {}", extra.name, distro.name);
        response.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, on, &label));
        if response.changed() {
            change = Some((extra.id.clone(), on));
        }
        ui.indent(("extra-detail", &extra.id), |ui| {
            ui.label(caption(offered.summary(), p));
            if let Some(needs) = &offered.provision.needs {
                ui.label(caption(format!("Requires {needs}"), p));
            }
        });
        ui.add_space(10.0);
    }
    if !built_in.is_empty() {
        let text = format!("Built into {}: {}", distro.name, built_in.join(", "));
        tag(ui, p.wash, Stroke::NONE, RichText::new(text).size(size::TINY).color(p.accent));
    }
    change
}

/// The way into composing a desktop from an edition that ships one: easy to miss
/// otherwise, since it lives on Minimal and most people pick a desktop edition first.
pub fn compose_offer(
    ui: &mut Ui,
    p: &Palette,
    id: impl egui::AsIdSalt,
    title: &str,
    teaser: &str,
) -> Response {
    choice_card(ui, p, id, title, false, true, |ui| {
        ui.horizontal(|ui| {
            glyph(ui, icon::WINDOW, size::HEADING, p.accent);
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width() - 24.0);
                ui.label(RichText::new(title).size(size::SMALL).strong());
                ui.add(egui::Label::new(caption(teaser, p)).wrap());
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(icon::CHEVRON).color(p.accent))
            });
        });
    })
}

/// A Nerd Font icon in a row. Its glyph is wider than its advance, so it gets the
/// room it needs before whatever follows.
pub fn glyph(ui: &mut Ui, icon: &str, size: f32, color: egui::Color32) {
    ui.label(RichText::new(icon).size(size).color(color));
    ui.add_space(size * 0.35);
}

/// The one strong action on a surface.
pub fn primary(ui: &mut Ui, p: &Palette, text: &str, full_width: bool) -> Response {
    let mut button = Button::new(RichText::new(text).color(p.bg).size(size::BODY))
        .fill(p.ink)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(RADIUS))
        .min_size(egui::vec2(0.0, 42.0));
    if full_width {
        button = button.min_size(egui::vec2(ui.available_width(), 42.0));
    }
    named(ui.add(button), text)
}

/// Announce a button by its words alone, without its icons.
fn named(response: Response, text: &str) -> Response {
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, icon::plain(text)));
    response
}

/// A quiet, frameless action such as "Skip · no desktop".
pub fn quiet(ui: &mut Ui, p: &Palette, text: &str) -> Response {
    let button = Button::new(RichText::new(text).color(p.sub).underline().size(size::SMALL));
    named(ui.add(button.frame(false)), text)
}

/// Small spaced capitals, the concept's eyebrow and field labels.
pub fn eyebrow(text: &str, color: egui::Color32) -> RichText {
    RichText::new(text.to_uppercase()).size(size::TINY).extra_letter_spacing(1.6).color(color)
}

pub fn caption(text: impl Into<String>, p: &Palette) -> RichText {
    RichText::new(text).size(size::SMALL).color(p.sub)
}

/// Label/value rows with a hairline between them.
pub fn rows(ui: &mut Ui, p: &Palette, rows: &[(&str, String)]) {
    for (i, (label, value)) in rows.iter().enumerate() {
        if i > 0 {
            let y = ui.cursor().top() - 3.0;
            let x = ui.max_rect().x_range();
            ui.painter().hline(x, y, Stroke::new(1.0, p.line.gamma_multiply(0.8)));
        }
        ui.horizontal(|ui| {
            ui.label(caption(*label, p));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(egui::Label::new(RichText::new(value).size(size::SMALL)).truncate());
            });
        });
    }
}

/// Two or more mutually exclusive options in a pill, e.g. Compose a desktop | No desktop.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    p: &Palette,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    Frame::new()
        .fill(p.wash)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            let width =
                (ui.available_width() - 4.0 * (options.len() as f32 - 1.0)) / options.len() as f32;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (option, label) in options {
                    let on = *value == *option;
                    let text = RichText::new(*label).size(size::SMALL).color(if on {
                        p.ink
                    } else {
                        p.sub
                    });
                    let button = Button::new(text)
                        .fill(if on { p.paper } else { egui::Color32::TRANSPARENT })
                        .stroke(if on { Stroke::new(1.0, p.line) } else { Stroke::NONE })
                        .corner_radius(CornerRadius::same(RADIUS - 2))
                        .min_size(egui::vec2(width, 30.0));
                    let response = ui.add(button);
                    response.widget_info(|| {
                        WidgetInfo::selected(Role::RadioButton, true, on, icon::plain(label))
                    });
                    if response.clicked() && !on {
                        *value = *option;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// A quiet note on a washed background.
pub fn notice(ui: &mut Ui, p: &Palette, text: &str) {
    Frame::new()
        .fill(p.wash)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(caption(text, p));
        });
}

/// A modal dialog in the house style. `id` keeps dialogs apart.
pub fn dialog<R>(
    ctx: &egui::Context,
    id: &str,
    p: &Palette,
    width: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::ModalResponse<R> {
    let frame = Frame::new()
        .fill(p.paper)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(CornerRadius::same(RADIUS + 2))
        .inner_margin(Margin::same(26));
    egui::Modal::new(egui::Id::unique(("borea-dialog", id))).frame(frame).show(ctx, |ui| {
        ui.set_width(width.min(ctx.content_rect().width() - 32.0));
        add(ui)
    })
}

/// A dialog's title with its Close button. Returns whether Close was pressed.
pub fn dialog_head(ui: &mut Ui, p: &Palette, title: &str) -> bool {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(size::TITLE));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let close = ui.add(
                Button::new(RichText::new(icon::CLOSE).size(size::HEADING).color(p.sub))
                    .frame(false),
            );
            close.widget_info(|| WidgetInfo::labeled(Role::Button, true, "Close"));
            close.clicked()
        })
        .inner
    })
    .inner
}

/// The icon for a machine fact's label (see `model::hardware_facts`).
pub fn fact_icon(label: &str) -> &'static str {
    match label {
        "Processor" => icon::CPU,
        "Graphics" => icon::GPU,
        "Memory" => icon::MEMORY,
        "Firmware" => icon::SHIELD,
        "Device" => icon::LAPTOP,
        "Display" => icon::MONITOR,
        "Disk" => icon::DISK,
        "Network" => icon::ETHERNET,
        "Bluetooth" => icon::BLUETOOTH,
        "Input" => icon::KEYBOARD,
        _ => icon::CHIP,
    }
}

/// "What Borea will do" for this machine: microcode, hybrid graphics, blockers.
pub fn findings(ui: &mut Ui, p: &Palette, hw: &borea_hardware::HardwareSnapshot) {
    let findings = borea_distro::machine_findings(hw);
    if findings.is_empty() {
        return;
    }
    ui.add_space(14.0);
    ui.label(eyebrow("What Borea will do", p.sub));
    ui.add_space(6.0);
    for finding in findings {
        let (mark, text) = match finding {
            borea_distro::Finding::Recommend(text) => (icon::NEXT, text),
            borea_distro::Finding::Unknown(text) => ("?", text),
            borea_distro::Finding::Blocker(text) => (icon::CLOSE, text),
        };
        ui.label(RichText::new(format!("{mark}  {text}")).size(size::SMALL));
    }
}
