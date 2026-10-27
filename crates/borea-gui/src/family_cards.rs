//! Portrait family cards, following the hero-select treatment in Claude's prototype.
use borea_core::Family;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Response, Shape, Stroke, Ui,
};

use crate::{
    icon,
    theme::{self, Palette, size},
};

struct Identity {
    accent: Color32,
    release: &'static str,
    description: &'static str,
}

fn identity(family: Family, dark: bool) -> Identity {
    let (dark_accent, light_accent, release, description) = match family {
        Family::Arch => (
            [86, 190, 233],
            [0, 104, 153],
            "Rolling release",
            "A flexible foundation to build your own way.",
        ),
        Family::Fedora => (
            [133, 174, 244],
            [42, 82, 155],
            "Regular releases",
            "New ideas, with a regular release rhythm.",
        ),
        Family::Debian => (
            [244, 135, 162],
            [161, 36, 74],
            "Stable release",
            "A dependable foundation for the long run.",
        ),
        Family::Opensuse => (
            [128, 210, 149],
            [34, 112, 57],
            "Rolling snapshots",
            "Fresh Tumbleweed snapshots, tested together.",
        ),
    };
    let [r, g, b] = if dark { dark_accent } else { light_accent };
    Identity { accent: Color32::from_rgb(r, g, b), release, description }
}

pub fn card(
    ui: &mut Ui,
    palette: &Palette,
    family: Family,
    distributions: &[&str],
    selected: bool,
    motion: bool,
) -> Response {
    let identity = identity(family, palette.dark);
    let accent = identity.accent;
    let response = ui.add_sized(
        egui::vec2(ui.available_width(), 378.0),
        egui::Button::new("").frame(false).selected(selected),
    );
    let active = response.hovered() || response.has_focus() || selected;
    let emphasis = if motion {
        ui.ctx().animate_bool_with_time(response.id.with("hero-emphasis"), active, 0.18)
    } else if active {
        1.0
    } else {
        0.0
    };
    // Reserve the lift inside the allocation, so neighboring cards never move.
    let rect =
        response.rect.shrink2(egui::vec2(2.0, 9.0)).translate(egui::vec2(0.0, -5.0 * emphasis));
    let painter = ui.painter_at(rect);
    let radius = CornerRadius::same(12);
    let stroke = if active { Stroke::new(1.7, accent) } else { Stroke::new(1.0, palette.line) };
    painter.rect_filled(rect, radius, palette.paper.lerp_to_gamma(accent, 0.035));
    painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
    if active {
        painter.rect_stroke(
            rect.shrink(4.0),
            CornerRadius::same(9),
            Stroke::new(1.0, accent.gamma_multiply(0.15)),
            egui::StrokeKind::Inside,
        );
    }
    let center = egui::pos2(rect.center().x, rect.top() + 92.0);
    for ring in (1..=12).rev() {
        let radius = 14.0 + ring as f32 * 5.1;
        painter.circle_filled(center, radius, accent.gamma_multiply(0.006 + emphasis * 0.004));
    }
    let diamond = [
        center + egui::vec2(0.0, -63.0),
        center + egui::vec2(63.0, 0.0),
        center + egui::vec2(0.0, 63.0),
        center + egui::vec2(-63.0, 0.0),
    ];
    painter
        .add(Shape::closed_line(diamond.to_vec(), Stroke::new(1.0, accent.gamma_multiply(0.25))));
    painter.text(
        center,
        Align2::CENTER_CENTER,
        icon::family(family),
        FontId::new(78.0, theme::icons()),
        accent,
    );
    painter.text(
        egui::pos2(rect.left() + 16.0, rect.top() + 15.0),
        Align2::LEFT_TOP,
        "LINUX FAMILY",
        FontId::new(9.0, theme::display()),
        palette.sub,
    );
    if selected {
        painter.text(
            egui::pos2(rect.right() - 16.0, rect.top() + 14.0),
            Align2::RIGHT_TOP,
            icon::CHECK,
            FontId::new(14.0, theme::icons()),
            accent,
        );
    }
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 169.0),
        Align2::CENTER_TOP,
        family.to_string(),
        FontId::new(26.0, theme::display()),
        palette.ink,
    );
    painter.text(
        egui::pos2(rect.center().x, rect.top() + 204.0),
        Align2::CENTER_TOP,
        identity.release.to_uppercase(),
        FontId::new(10.0, theme::display()),
        accent,
    );
    let content = rect.shrink2(egui::vec2(17.0, 0.0));
    text_block(
        &painter,
        egui::pos2(content.left(), rect.top() + 230.0),
        content.width(),
        identity.description,
        size::SMALL,
        palette.sub,
    );
    let names = distributions.join(" · ");
    text_block(
        &painter,
        egui::pos2(content.left(), rect.top() + 279.0),
        content.width(),
        &names,
        10.0,
        palette.sub,
    );
    let line_y = rect.bottom() - 42.0;
    painter.hline(content.x_range(), line_y, Stroke::new(1.0, accent.gamma_multiply(0.2)));
    painter.text(
        egui::pos2(content.left(), rect.bottom() - 23.0),
        Align2::LEFT_CENTER,
        "Explore family",
        FontId::new(11.0, theme::display()),
        accent,
    );
    painter.text(
        egui::pos2(content.right(), rect.bottom() - 23.0),
        Align2::RIGHT_CENTER,
        icon::NEXT,
        FontId::new(15.0, theme::icons()),
        accent,
    );
    if response.has_focus() {
        // Focus also has an inset marker, so keyboard use is clear without color.
        painter.rect_stroke(
            rect.shrink(7.0),
            CornerRadius::same(6),
            Stroke::new(1.0, accent),
            egui::StrokeKind::Inside,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::accesskit::Role::Button,
            true,
            selected,
            format!("{family} family"),
        )
    });
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_description(format!(
            "{}. {} Distributions: {}.",
            identity.release, identity.description, names
        ));
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn text_block(
    painter: &egui::Painter,
    position: Pos2,
    width: f32,
    text: &str,
    size: f32,
    color: Color32,
) {
    let galley = painter.layout(
        text.to_owned(),
        FontId::new(size, egui::FontFamily::Proportional),
        color,
        width,
    );
    painter.galley(position, galley, color);
}
