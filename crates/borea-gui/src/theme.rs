//! Windpad's look: Nord's snow storm or polar night, frost lines, a cool frost-blue accent,
//! set in JetBrains Mono Nerd Font with its icons. The colour tokens match the HTML
//! concept, so both read as one design.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg: Color32,
    pub paper: Color32,
    pub ink: Color32,
    pub sub: Color32,
    pub line: Color32,
    pub accent: Color32,
    pub wash: Color32,
    pub dark: bool,
}

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xec, 0xef, 0xf4),
    paper: Color32::from_rgb(0xf6, 0xf8, 0xfb),
    ink: Color32::from_rgb(0x2e, 0x34, 0x40),
    sub: Color32::from_rgb(0x4c, 0x56, 0x6a),
    line: Color32::from_rgb(0xd8, 0xde, 0xe9),
    accent: Color32::from_rgb(0x45, 0x68, 0x90),
    wash: Color32::from_rgb(0xdf, 0xe6, 0xef),
    dark: false,
};

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x2e, 0x34, 0x40),
    paper: Color32::from_rgb(0x3b, 0x42, 0x52),
    ink: Color32::from_rgb(0xec, 0xef, 0xf4),
    sub: Color32::from_rgb(0xb4, 0xbc, 0xcb),
    line: Color32::from_rgb(0x4c, 0x56, 0x6a),
    accent: Color32::from_rgb(0x88, 0xc0, 0xd0),
    wash: Color32::from_rgb(0x3d, 0x4a, 0x5c),
    dark: true,
};

/// Text sizes, named by role rather than by number.
pub mod size {
    pub const HERO: f32 = 44.0;
    pub const TITLE: f32 = 22.0;
    pub const HEADING: f32 = 18.0;
    pub const BODY: f32 = 14.0;
    pub const SMALL: f32 = 12.0;
    pub const TINY: f32 = 11.0;
}

pub const RADIUS: u8 = 6;

/// Every word and icon: JetBrains Mono Nerd Font, built in (see `fonts/README.md`).
const NERD: &[u8] = include_bytes!("../fonts/JetBrainsMonoNerdFont-Regular.ttf");
const NERD_BOLD: &[u8] = include_bytes!("../fonts/JetBrainsMonoNerdFont-Bold.ttf");

/// System fonts after it, for scripts it does not cover: Noto where the system has
/// it, else DejaVu (which Borea Live ships).
const FALLBACK: &[&str] = &[
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/google-noto-vf/NotoSans[wght].ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

/// Bold, for titles and the hero.
pub fn display() -> FontFamily {
    FontFamily::Name("display".into())
}

/// The Nerd Font alone, for checking icons.
pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}

pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("nerd".into(), FontData::from_static(NERD).into());
    fonts.font_data.insert("nerd-bold".into(), FontData::from_static(NERD_BOLD).into());
    let fallback = FALLBACK.iter().find_map(|path| std::fs::read(path).ok()).map(|bytes| {
        fonts.font_data.insert("fallback".into(), FontData::from_owned(bytes).into());
        "fallback".to_owned()
    });
    let builtin = fonts.families[&FontFamily::Proportional].clone();
    let chain = |first: &str| -> Vec<String> {
        std::iter::once(first.to_owned()).chain(fallback.clone()).chain(builtin.clone()).collect()
    };
    fonts.families.insert(FontFamily::Proportional, chain("nerd"));
    fonts.families.insert(FontFamily::Monospace, chain("nerd"));
    fonts.families.insert(display(), chain("nerd-bold"));
    fonts.families.insert(icons(), vec!["nerd".into()]);
    fonts
}

pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut visuals = if p.dark { Visuals::dark() } else { Visuals::light() };
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.paper;
    visuals.extreme_bg_color = p.bg;
    visuals.faint_bg_color = p.wash;
    visuals.override_text_color = Some(p.ink);
    visuals.hyperlink_color = p.accent;
    visuals.window_stroke = Stroke::new(1.0, p.line);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.menu_corner_radius = CornerRadius::same(6);
    visuals.selection.bg_fill = p.accent.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, p.accent);
    visuals.window_shadow.color = Color32::from_black_alpha(if p.dark { 70 } else { 18 });
    visuals.popup_shadow.color = visuals.window_shadow.color;

    let widgets = &mut visuals.widgets;
    for (state, fill, stroke) in [
        (&mut widgets.noninteractive, p.paper, p.line),
        (&mut widgets.inactive, p.bg, p.line),
        (&mut widgets.hovered, p.bg, p.accent),
        (&mut widgets.active, p.wash, p.accent),
        (&mut widgets.open, p.bg, p.accent),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0, stroke);
        state.fg_stroke = Stroke::new(1.0, p.ink);
        state.corner_radius = CornerRadius::same(4);
        state.expansion = 0.0;
    }
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.ink);

    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        style.interaction.selectable_labels = false;
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(14.0, 9.0);
        style.spacing.interact_size.y = 34.0;
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(size::TITLE)),
            (TextStyle::Body, FontId::proportional(size::BODY)),
            (TextStyle::Button, FontId::proportional(size::BODY)),
            (TextStyle::Small, FontId::proportional(size::TINY)),
            (TextStyle::Monospace, FontId::monospace(size::SMALL)),
        ]
        .into();
    });
}
