//! Noctalia v5 compositor recipes. Package sources were audited 2026-10-31.
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
            assert!(groups::desktop_unavailable(Distro::Arch, recipe.desktop).is_none());
            for distro in [Distro::Debian, Distro::Fedora, Distro::OpenSuse] {
                assert!(groups::desktop_unavailable(distro, recipe.desktop).is_some());
            }
            let selected = groups::plan_groups("composed", &[], recipe.desktop, Distro::Arch)?;
            assert_eq!(selected.desktop, Some((recipe.package, "greetd")));
        }
        assert!(compositor("sway").is_none());
        assert!(recipe("niri").is_none());
        Ok(())
    }
}
