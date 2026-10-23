//! Nerd Font icons the interface uses, by meaning. Each is one glyph of the bundled
//! JetBrains Mono Nerd Font; names follow nerdfonts.com's cheat sheet.

use borea_core::Family;

// Distributions and desktops (nf-linux-*).
pub const ARCH: &str = "\u{f303}";
pub const CACHYOS: &str = "\u{f385}";
pub const FEDORA: &str = "\u{f30a}";
pub const NOBARA: &str = "\u{f380}";
pub const DEBIAN: &str = "\u{f306}";
pub const OPENSUSE: &str = "\u{f314}";
pub const TUMBLEWEED: &str = "\u{f37d}";
pub const TUX: &str = "\u{f31a}";
pub const HYPRLAND: &str = "\u{f359}";
pub const FLATHUB: &str = "\u{f324}";

// Machine (nf-md-*, nf-oct-cpu).
pub const CPU: &str = "\u{f4bc}";
pub const GPU: &str = "\u{f08ae}";
pub const MEMORY: &str = "\u{f035b}";
pub const CHIP: &str = "\u{f061a}";
pub const DISK: &str = "\u{f02ca}";
pub const MONITOR: &str = "\u{f0379}";
pub const LAPTOP: &str = "\u{f0322}";
pub const WIFI: &str = "\u{f05a9}";
pub const WIFI_OFF: &str = "\u{f05aa}";
pub const ETHERNET: &str = "\u{f0200}";
pub const SHIELD: &str = "\u{f0565}";

// Desktop and extras.
pub const WINDOW: &str = "\u{f10ac}";
pub const DASHBOARD: &str = "\u{f056e}";
pub const CONSOLE: &str = "\u{f018d}";
pub const GAMEPAD: &str = "\u{f0297}";
pub const FILM: &str = "\u{f0230}";
pub const PRINTER: &str = "\u{f042a}";
pub const PACKAGE: &str = "\u{f03d6}";

// Marks.
pub const CHECK: &str = "\u{f012c}";
pub const CLOSE: &str = "\u{f0156}";
pub const NEXT: &str = "\u{f19b0}";
pub const BACK: &str = "\u{f19b1}";
pub const OPEN: &str = "\u{f19b4}";
pub const CHEVRON: &str = "\u{f0142}";
pub const STAR: &str = "\u{f04ce}";
pub const DOT: &str = "\u{f0765}";
pub const RING: &str = "\u{f0766}";
pub const THEME: &str = "\u{f050e}";
pub const WIND: &str = "\u{f059d}";
pub const ONE: &str = "\u{f0ca1}";
pub const TWO: &str = "\u{f0ca3}";

pub fn family(family: Family) -> &'static str {
    match family {
        Family::Arch => ARCH,
        Family::Fedora => FEDORA,
        Family::Debian => DEBIAN,
        Family::Opensuse => OPENSUSE,
    }
}

/// A distribution's own logo where Nerd Fonts has one, else Tux.
pub fn distro(id: &str) -> &'static str {
    match id {
        "arch" => ARCH,
        "cachyos" => CACHYOS,
        "fedora" => FEDORA,
        "nobara" => NOBARA,
        "debian" => DEBIAN,
        "tumbleweed" => TUMBLEWEED,
        _ => TUX,
    }
}

pub fn compositor(id: &str) -> &'static str {
    match id {
        "hyprland" => HYPRLAND,
        "niri" => DASHBOARD,
        _ => WINDOW,
    }
}

pub fn extra(id: &str) -> &'static str {
    match id {
        "gaming" => GAMEPAD,
        "codecs" => FILM,
        "flatpak" => FLATHUB,
        "printing" => PRINTER,
        _ => PACKAGE,
    }
}

/// `text` without its icons, for accessible names: screen readers have nothing to
/// say for a private-use glyph.
pub fn plain(text: &str) -> String {
    let private = |c: &char| matches!(*c as u32, 0xe000..=0xf8ff | 0xf0000..=0x10ffff);
    text.chars()
        .filter(|c| !private(c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every icon above, for the test that the bundled font has them all.
#[cfg(test)]
pub const ALL: &[&str] = &[
    ARCH, CACHYOS, FEDORA, NOBARA, DEBIAN, OPENSUSE, TUMBLEWEED, TUX, HYPRLAND, FLATHUB, CPU, GPU,
    MEMORY, CHIP, DISK, MONITOR, LAPTOP, WIFI, WIFI_OFF, ETHERNET, SHIELD, WINDOW, DASHBOARD,
    CONSOLE, GAMEPAD, FILM, PRINTER, PACKAGE, CHECK, CLOSE, NEXT, BACK, OPEN, CHEVRON, STAR, DOT,
    RING, THEME, WIND, ONE, TWO,
];

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{FontId, epaint::text::Fonts, epaint::text::TextOptions};

    #[test]
    fn the_bundled_font_has_every_icon() {
        let mut fonts = Fonts::new(TextOptions::default(), crate::theme::definitions());
        let icons = FontId::new(14.0, crate::theme::icons());
        for icon in ALL {
            let c = icon.chars().next().unwrap();
            assert!(fonts.has_glyph(&icons, c), "U+{:X} is missing", c as u32);
        }
    }

    #[test]
    fn accessible_names_drop_the_icons() {
        assert_eq!(plain(&format!("Next {NEXT}")), "Next");
        assert_eq!(plain(&format!("{WIFI} Online")), "Online");
        assert_eq!(plain("Skip · no desktop"), "Skip · no desktop");
    }
}
