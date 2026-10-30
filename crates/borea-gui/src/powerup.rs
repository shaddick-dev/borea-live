//! Power-up cards: choices you pick up, as in a game. An emblem floats in a
//! glowing orb; hovering lifts the glow, sends a shine across the card and sets
//! sparks orbiting the orb; picking one pops the orb with a ring burst and keeps
//! it lit. Everything is painted by egui (no images, no shaders), so it stays
//! cheap on software rendering. `time` is `None` when motion is paused or the
//! window is unfocused: then the card is still, with the same colours.

use std::f32::consts::TAU;

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Frame, Layout, Margin, Pos2, Rect, Response, Role,
    Sense, Shadow, Shape, Stroke, Ui, Vec2, WidgetInfo,
};

use crate::theme::{self, Palette, RADIUS};

/// How long the pick-up burst lasts, in seconds.
const BURST: f64 = 0.55;

/// How a power-up card is drawn.
pub struct PowerUp<'a> {
    /// What screen readers and tests call it.
    pub label: &'a str,
    /// One icon glyph from the bundled Nerd Font.
    pub emblem: &'a str,
    /// Diameter of the orb around the emblem.
    pub orb: f32,
    pub selected: bool,
    /// Where it sits in its row: staggers the idle float so cards don't bob together.
    pub index: usize,
}

fn with_alpha(c: Color32, a: f32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// A four-pointed spark.
fn spark(painter: &egui::Painter, at: Pos2, size: f32, color: Color32) {
    let s = Stroke::new(1.4, color);
    painter.line_segment([at - Vec2::new(size, 0.0), at + Vec2::new(size, 0.0)], s);
    painter.line_segment([at - Vec2::new(0.0, size), at + Vec2::new(0.0, size)], s);
}

/// A card that acts as one button. `add_contents` draws the text under the orb.
pub fn card(
    ui: &mut Ui,
    p: &Palette,
    id: impl egui::AsIdSalt,
    look: &PowerUp<'_>,
    time: Option<f64>,
    add_contents: impl FnOnce(&mut Ui),
) -> Response {
    let id = ui.scope_id().with(id);
    let ctx = ui.ctx().clone();
    let was_hovered = ctx.read_response(id).is_some_and(|r| r.hovered());
    let hover = ctx.animate_bool_with_time(id.with("hover"), was_hovered, 0.18);
    let picked = ctx.animate_bool_with_time(id.with("picked"), look.selected, 0.25);
    let lit = hover.max(picked);
    let burst = ctx
        .data(|d| d.get_temp::<f64>(id.with("burst")))
        .and_then(|start| time.map(|now| ((now - start) / BURST) as f32))
        .filter(|t| (0.0..1.0).contains(t));

    let glow = Shadow {
        offset: [0, (2.0 + 4.0 * lit) as i8],
        blur: (6.0 + 18.0 * lit) as u8,
        spread: (2.0 * lit) as u8,
        color: with_alpha(p.accent, 0.10 + 0.30 * lit),
    };
    let border = Stroke::new(1.0 + 1.0 * picked, p.line.lerp_to_gamma(p.accent, lit));
    let card = Frame::new()
        .fill(p.paper)
        .stroke(border)
        .shadow(glow)
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                let (orb_rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), look.orb + 16.0),
                    Sense::hover(),
                );
                orb(ui, p, orb_rect, look, time, lit, picked, burst);
                ui.add_space(6.0);
                add_contents(ui);
            });
        });

    let rect = card.response.rect;
    if let Some(t) = time.filter(|_| hover > 0.01) {
        shine(ui, rect, t, hover, p);
    }

    // Only the visible part takes clicks (see widgets::choice_card).
    let visible = rect.intersect(ui.clip_rect());
    let (area, sense) = if visible.is_positive() {
        (visible, Sense::click())
    } else {
        (Rect::from_min_size(rect.min, Vec2::ZERO), Sense::hover())
    };
    let response = ui.interact(area, id, sense);
    if response.clicked() {
        let now = ctx.input(|i| i.time);
        ctx.data_mut(|d| d.insert_temp(id.with("burst"), now));
    }
    if response.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if time.is_some() && (lit > 0.0 || burst.is_some()) {
        ctx.request_repaint();
    }
    response.widget_info(|| WidgetInfo::selected(Role::Button, true, look.selected, look.label));
    response
}

#[allow(clippy::too_many_arguments)]
fn orb(
    ui: &Ui,
    p: &Palette,
    area: Rect,
    look: &PowerUp<'_>,
    time: Option<f64>,
    lit: f32,
    picked: f32,
    burst: Option<f32>,
) {
    let painter = ui.painter();
    let t = time.unwrap_or(0.0) as f32;
    let phase = look.index as f32 * 1.3;
    // Idle float: a slow bob, a little higher when lit.
    let bob = if time.is_some() { (t * 2.0 + phase).sin() * (2.0 + 1.5 * lit) } else { 0.0 };
    let center = area.center() + Vec2::new(0.0, bob - 2.0 * lit);
    let r = look.orb / 2.0;

    // Halo, then the orb, then its rim.
    for (k, alpha) in [(1.45, 0.06), (1.22, 0.10)] {
        painter.circle_filled(center, r * k, with_alpha(p.accent, alpha * (0.6 + lit)));
    }
    painter.circle_filled(
        center,
        r,
        p.wash.lerp_to_gamma(with_alpha(p.accent, 1.0), 0.18 * picked),
    );
    painter.circle_stroke(
        center,
        r,
        Stroke::new(1.0 + picked, with_alpha(p.accent, 0.35 + 0.65 * lit)),
    );

    // Pick-up pop: the emblem swells and settles, a ring bursts outward and fades.
    let pop = burst.map_or(0.0, |b| (b * std::f32::consts::PI).sin());
    let size = look.orb * 0.48 * (1.0 + 0.08 * lit + 0.22 * pop);
    let ink = p.sub.lerp_to_gamma(p.accent, lit.max(0.4));
    emblem(painter, center, look.emblem, size, ink);
    if let Some(b) = burst {
        let ease = 1.0 - (1.0 - b).powi(3);
        painter.circle_stroke(
            center,
            r * (1.0 + 0.9 * ease),
            Stroke::new(3.0 * (1.0 - b), with_alpha(p.accent, 1.0 - b)),
        );
        for k in 0..8 {
            let a = k as f32 / 8.0 * TAU;
            let at = center + Vec2::angled(a) * r * (1.05 + 0.85 * ease);
            spark(painter, at, 3.5 * (1.0 - b), with_alpha(p.accent, 1.0 - b));
        }
    }

    // Sparks orbit the orb while it is lit.
    if let Some(time) = time.filter(|_| lit > 0.01) {
        let t = time as f32;
        for k in 0..3 {
            let a = t * 0.9 + k as f32 * TAU / 3.0 + phase;
            let at = center + Vec2::new(a.cos() * r * 1.3, a.sin() * r * 0.55);
            let twinkle = 0.5 + 0.5 * (t * 5.0 + k as f32 * 2.0).sin();
            spark(
                painter,
                at,
                2.0 + 1.5 * twinkle,
                with_alpha(p.accent, lit * (0.4 + 0.6 * twinkle)),
            );
        }
    }
}

/// A band of light sweeping across the card.
fn shine(ui: &Ui, rect: Rect, time: f64, strength: f32, p: &Palette) {
    let painter = ui.painter().with_clip_rect(rect.shrink(1.0).intersect(ui.clip_rect()));
    let span = rect.width() + rect.height();
    let x = rect.left() - rect.height() + ((time * 0.45).fract() as f32) * (span + rect.height());
    let w = 36.0;
    let color = with_alpha(if p.dark { Color32::WHITE } else { p.accent }, 0.06 * strength);
    painter.add(Shape::convex_polygon(
        vec![
            Pos2::new(x, rect.bottom()),
            Pos2::new(x + w, rect.bottom()),
            Pos2::new(x + w + rect.height(), rect.top()),
            Pos2::new(x + rect.height(), rect.top()),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Draw a glyph with its visible ink, not its text box, centred on `center`:
/// icon glyphs sit off-centre in their line box (baseline, side bearings).
fn emblem(painter: &egui::Painter, center: Pos2, glyph: &str, size: f32, color: Color32) {
    let galley = painter.layout_no_wrap(glyph.to_owned(), FontId::new(size, theme::icons()), color);
    let ink = galley.rows.first().and_then(|row| {
        row.glyphs.first().filter(|g| g.uv_rect.size.x > 0.0).map(|g| {
            Rect::from_min_size(row.pos + g.pos.to_vec2() + g.uv_rect.offset, g.uv_rect.size)
        })
    });
    let origin = match ink {
        Some(ink) => center - ink.center().to_vec2(),
        None => center - galley.size() / 2.0,
    };
    painter.galley(origin, galley, color);
}
