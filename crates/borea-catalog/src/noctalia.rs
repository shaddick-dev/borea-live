//! Noctalia v5 compositor recipes. Arch sources were audited 2026-10-31,
//! Tumbleweed's 2026-11-01 (see backend/docs/VERIFIED.md).
use crate::Distro;

/// Families with a native Noctalia v5 install.
pub const DISTROS: [Distro; 2] = [Distro::Arch, Distro::OpenSuse];
pub fn supported(distro: Distro) -> bool {
    DISTROS.contains(&distro)
}
/// Packages every Noctalia v5 install needs before the first graphical login,
/// whatever the compositor. Optional apps may finish after login.
pub fn base_packages(distro: Distro) -> &'static [&'static str] {
    match distro {
        Distro::OpenSuse => &[
            "noctalia",
            "noctalia-greeter",
            "greetd",
            "accountsservice",
            "xdg-desktop-portal-gtk",
            "xdg-user-dirs",
            "pipewire",
            "pipewire-pulseaudio",
            "wireplumber",
            "polkit",
            "dbus-1",
            "dejavu-fonts",
            "foot",
            "plymouth",
            "plymouth-dracut",
            "plymouth-theme-spinner",
        ],
        _ => crate::groups::ARCH_NOCTALIA_PACKAGES,
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Recipe {
    pub compositor: &'static str,
    pub desktop: &'static str,
    pub name: &'static str,
    pub package: &'static str,
    pub packages: &'static [&'static str],
    pub session: &'static str,
    pub desktop_name: &'static str,
    pub portal: &'static str,
}
pub const RECIPES: &[Recipe] = &[
    Recipe {
        compositor: "hyprland",
        desktop: "hyprland-noctalia",
        name: "Hyprland",
        package: "hyprland",
        packages: &["xdg-desktop-portal-hyprland"],
        session: "/usr/bin/start-hyprland",
        desktop_name: "Hyprland",
        portal: "hyprland",
    },
    Recipe {
        compositor: "niri",
        desktop: "niri-noctalia",
        name: "niri",
        package: "niri",
        packages: &["xdg-desktop-portal-gnome", "xwayland-satellite", "playerctl"],
        session: "/usr/bin/niri-session",
        desktop_name: "niri",
        portal: "gnome",
    },
    Recipe {
        compositor: "mango",
        desktop: "mango-noctalia",
        name: "MangoWC",
        package: "mangowm",
        packages: &["xdg-desktop-portal-wlr"],
        session: "/usr/bin/mango",
        desktop_name: "mango",
        portal: "wlr",
    },
];
impl Recipe {
    /// The compositor package on this family, and the repository it comes from.
    /// Tumbleweed names match Arch's except MangoWC, which comes from its OBS project.
    pub fn compositor_package(&self, distro: Distro) -> Option<(&'static str, &'static str)> {
        let base = crate::repos_for(distro).find(|r| r.base)?.id;
        match (distro, self.compositor) {
            (Distro::Arch, _) => Some((base, self.package)),
            (Distro::OpenSuse, "mango") => Some(("opensuse.mangowc", "mangowc")),
            (Distro::OpenSuse, _) => Some((base, self.package)),
            _ => None,
        }
    }
}
pub fn recipe(desktop: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| r.desktop == desktop)
}
pub fn compositor(id: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| r.compositor == id)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recipes_resolve_each_native_desktop_without_cross_installing_compositors()
    -> Result<(), crate::GroupError> {
        use crate::{Distro, groups};
        for recipe in RECIPES {
            assert_eq!(compositor(recipe.compositor).map(|r| r.desktop), Some(recipe.desktop));
            for distro in DISTROS {
                assert!(groups::desktop_unavailable(distro, recipe.desktop).is_none());
                let package = recipe.compositor_package(distro).map(|(_, name)| name);
                assert!(package.is_some());
                let selected = groups::plan_groups("composed", &[], recipe.desktop, distro)?;
                assert_eq!(selected.desktop.map(|(name, _)| name), package);
                assert_eq!(selected.desktop.map(|(_, manager)| manager), Some("greetd"));
            }
            for distro in [Distro::Debian, Distro::Fedora] {
                assert!(groups::desktop_unavailable(distro, recipe.desktop).is_some());
                assert!(recipe.compositor_package(distro).is_none());
            }
        }
        assert_eq!(
            compositor("mango").and_then(|r| r.compositor_package(Distro::OpenSuse)),
            Some(("opensuse.mangowc", "mangowc"))
        );
        assert!(crate::source::repo("opensuse.mangowc").is_some_and(|r| r.on_demand && !r.base));
        assert!(compositor("sway").is_none());
        assert!(recipe("niri").is_none());
        Ok(())
    }
}
