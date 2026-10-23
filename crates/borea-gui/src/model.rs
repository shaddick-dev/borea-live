//! What the user has chosen on each family card, and what follows from it.
//!
//! Pure state over the catalog: no egui, no I/O. The window renders it; tests
//! drive it directly.

use std::collections::BTreeMap;

use borea_catalog::{Catalog, DesktopChoice, Distro, Edition, Offered, Source};
use borea_core::Family;

/// Whether a desktop-free edition gets a composed desktop or stays bare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopMode {
    Compose,
    Bare,
}

/// The shell step: a shell, or the compositor on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellPick {
    Shell(String),
    NoShell,
}

#[derive(Debug, Clone)]
pub struct FamilyChoice {
    pub distro: usize,
    pub edition: usize,
    pub variant: usize,
    pub mode: DesktopMode,
    pub compositor: Option<String>,
    pub shell: Option<ShellPick>,
    /// Set by "Use this desktop"; any change to the compose path clears it.
    pub confirmed: bool,
    /// Extras checkboxes the user has touched, by id; the rest keep the
    /// distribution's default.
    pub extras: BTreeMap<String, bool>,
}

impl Default for FamilyChoice {
    fn default() -> Self {
        FamilyChoice {
            distro: 0,
            edition: 0,
            variant: 0,
            mode: DesktopMode::Compose,
            compositor: None,
            shell: None,
            confirmed: false,
            extras: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Selection {
    pub choices: BTreeMap<Family, FamilyChoice>,
    pub selected: Family,
    /// Secure Boot for the installed system, with the owner's own keys. Off unless
    /// the user turns it on.
    pub secure_boot: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Selection {
            choices: Family::ALL.into_iter().map(|f| (f, FamilyChoice::default())).collect(),
            selected: Family::Arch,
            secure_boot: false,
        }
    }
}

impl Selection {
    pub fn choice(&self, family: Family) -> &FamilyChoice {
        &self.choices[&family]
    }

    fn choice_mut(&mut self, family: Family) -> &mut FamilyChoice {
        self.choices.get_mut(&family).expect("every family has a choice")
    }

    pub fn distro<'c>(&self, catalog: &'c Catalog, family: Family) -> &'c Distro {
        let index = self.choice(family).distro;
        catalog.family(family).nth(index).expect("choice indexes the family's distros")
    }

    pub fn edition<'c>(&self, catalog: &'c Catalog, family: Family) -> &'c Edition {
        &self.distro(catalog, family).editions[self.choice(family).edition]
    }

    /// The selected edition ships without a desktop, so the compose path applies.
    pub fn composable(&self, catalog: &Catalog, family: Family) -> bool {
        self.edition(catalog, family).composable
    }

    pub fn select(&mut self, family: Family) {
        self.selected = family;
    }

    pub fn set_distro(&mut self, family: Family, index: usize) {
        *self.choice_mut(family) = FamilyChoice { distro: index, ..FamilyChoice::default() };
        self.selected = family;
    }

    pub fn set_edition(&mut self, family: Family, index: usize) {
        let choice = self.choice_mut(family);
        choice.edition = index;
        choice.confirmed = false;
        self.selected = family;
    }

    pub fn set_variant(&mut self, family: Family, index: usize) {
        self.choice_mut(family).variant = index;
        self.selected = family;
    }

    pub fn set_mode(&mut self, family: Family, mode: DesktopMode) {
        self.choice_mut(family).mode = mode;
        self.selected = family;
    }

    /// Whether the extra's checkbox is ticked. Built-in extras have no checkbox.
    pub fn extra(&self, catalog: &Catalog, family: Family, id: &str) -> bool {
        let distro = &self.distro(catalog, family).id;
        catalog.extras.on(distro).find(|o| o.extra.id == id).is_some_and(|o| {
            !o.provision.built_in
                && self.choice(family).extras.get(id).copied().unwrap_or(o.provision.default)
        })
    }

    pub fn set_extra(&mut self, family: Family, id: &str, on: bool) {
        self.choice_mut(family).extras.insert(id.to_owned(), on);
        self.selected = family;
    }

    /// The extras this card will install.
    pub fn ticked<'c>(&self, catalog: &'c Catalog, family: Family) -> Vec<Offered<'c>> {
        let distro = &self.distro(catalog, family).id;
        catalog.extras.on(distro).filter(|o| self.extra(catalog, family, &o.extra.id)).collect()
    }

    /// Where this card can compose a desktop instead: its distribution's Minimal,
    /// else the first distribution in the family that has one, as (distribution,
    /// edition) indexes. None when the card already composes, or nothing can.
    pub fn compose_target(&self, catalog: &Catalog, family: Family) -> Option<(usize, usize)> {
        if self.composable(catalog, family) {
            return None;
        }
        let here = self.choice(family).distro;
        let distros: Vec<_> = catalog.family(family).collect();
        let order = std::iter::once(here).chain((0..distros.len()).filter(|i| *i != here));
        order
            .filter_map(|i| Some((i, distros[i].editions.iter().position(|e| e.composable)?)))
            .next()
    }

    /// Switch the card to composing its own desktop.
    pub fn compose_on(&mut self, family: Family, (distro, edition): (usize, usize)) {
        if self.choice(family).distro != distro {
            self.set_distro(family, distro);
        }
        self.set_edition(family, edition);
        self.set_mode(family, DesktopMode::Compose);
    }

    /// Pre-fill the compose path with sensible defaults: the first compositor the
    /// distribution packages, and the suggested shell for it.
    pub fn ensure_defaults(&mut self, catalog: &Catalog, family: Family) {
        if !self.composable(catalog, family) {
            return;
        }
        let distro = &self.distro(catalog, family).id;
        let desktops = &catalog.desktops;
        let choice = self.choice_mut(family);
        let available =
            |id: &str| desktops.compositors(distro).any(|o| o.available() && o.item.id == id);
        if !choice.compositor.as_deref().is_some_and(available) {
            choice.compositor =
                desktops.compositors(distro).find(|o| o.available()).map(|o| o.item.id.clone());
        }
        let Some(compositor) = choice.compositor.clone() else { return };
        let valid = match &choice.shell {
            Some(ShellPick::NoShell) => true,
            Some(ShellPick::Shell(id)) => {
                desktops.shells(distro, &compositor).any(|o| &o.item.id == id)
            }
            None => false,
        };
        if !valid {
            choice.shell = desktops
                .suggested(distro, &compositor)
                .map(|s| ShellPick::Shell(s.id.clone()))
                .or(Some(ShellPick::NoShell));
        }
    }

    pub fn set_compositor(&mut self, catalog: &Catalog, family: Family, id: &str) {
        let choice = self.choice_mut(family);
        choice.compositor = Some(id.to_owned());
        choice.confirmed = false;
        if choice.shell != Some(ShellPick::NoShell) {
            choice.shell = None;
        }
        self.ensure_defaults(catalog, family);
    }

    pub fn set_shell(&mut self, family: Family, pick: ShellPick) {
        let choice = self.choice_mut(family);
        choice.shell = Some(pick);
        choice.confirmed = false;
    }

    /// "Use this desktop".
    pub fn confirm(&mut self, family: Family) {
        let choice = self.choice_mut(family);
        choice.mode = DesktopMode::Compose;
        choice.confirmed = true;
    }

    /// The card's path is complete, so its preview and Continue appear: an edition
    /// with its own desktop, "No desktop", or a confirmed compositor and shell.
    pub fn path_complete(&self, catalog: &Catalog, family: Family) -> bool {
        let choice = self.choice(family);
        !self.composable(catalog, family) || choice.mode == DesktopMode::Bare || choice.confirmed
    }

    pub fn desktop_choice(&self, catalog: &Catalog, family: Family) -> Option<DesktopChoice> {
        if !self.composable(catalog, family) {
            return None;
        }
        let choice = self.choice(family);
        let compositor = choice.compositor.clone();
        Some(match (choice.mode, compositor, &choice.shell) {
            (DesktopMode::Bare, _, _) | (_, None, _) => DesktopChoice::None,
            (_, Some(compositor), Some(ShellPick::Shell(shell))) => {
                DesktopChoice::Shell { compositor, shell: shell.clone() }
            }
            (_, Some(compositor), _) => DesktopChoice::Compositor { compositor },
        })
    }

    pub fn compositor_name<'c>(&self, catalog: &'c Catalog, family: Family) -> Option<&'c str> {
        let id = self.choice(family).compositor.as_deref()?;
        catalog.desktops.compositors.iter().find(|c| c.id == id).map(|c| c.name.as_str())
    }

    pub fn shell_name<'c>(&self, catalog: &'c Catalog, family: Family) -> &'c str {
        match &self.choice(family).shell {
            Some(ShellPick::Shell(id)) => {
                catalog.desktops.shells.iter().find(|s| &s.id == id).map_or("", |s| s.name.as_str())
            }
            _ => "No shell",
        }
    }

    /// "Arch Linux · Minimal · niri + iNiR".
    pub fn title(&self, catalog: &Catalog, family: Family) -> String {
        let distro = self.distro(catalog, family);
        let mut title = format!("{} · {}", distro.name, self.edition(catalog, family).name);
        if let Some(variant) = distro.variants.get(self.choice(family).variant) {
            title.push_str(&format!(" · {variant}"));
        }
        match self.desktop_choice(catalog, family) {
            None => {}
            Some(DesktopChoice::None) => title.push_str(" · no desktop"),
            Some(DesktopChoice::Compositor { .. }) => {
                title.push_str(&format!(
                    " · {}",
                    self.compositor_name(catalog, family).unwrap_or("")
                ));
            }
            Some(DesktopChoice::Shell { .. }) => title.push_str(&format!(
                " · {} + {}",
                self.compositor_name(catalog, family).unwrap_or(""),
                self.shell_name(catalog, family)
            )),
        }
        title
    }

    /// The selection as label/value rows, for the card preview and the summary.
    pub fn rows(&self, catalog: &Catalog, family: Family) -> Vec<(&'static str, String)> {
        let distro = self.distro(catalog, family);
        let edition = self.edition(catalog, family);
        let mut rows =
            vec![("Distribution", distro.name.clone()), ("Edition", edition.name.clone())];
        if let Some(variant) = distro.variants.get(self.choice(family).variant) {
            rows.push(("Image variant", variant.clone()));
        }
        if let Some(choice) = self.desktop_choice(catalog, family) {
            match catalog.desktops.resolve(&distro.id, &choice) {
                Ok((None, _)) => rows.push(("Desktop", "None · console login".into())),
                Ok((Some(compositor), shell)) => {
                    let name = self.compositor_name(catalog, family).unwrap_or_default();
                    rows.push(("Compositor", format!("{name} · {}", compositor.badge())));
                    rows.push((
                        "Shell",
                        match shell {
                            Some(source) => {
                                format!("{} · {}", self.shell_name(catalog, family), source.badge())
                            }
                            None => "None · compositor only".into(),
                        },
                    ));
                }
                Err(error) => rows.push(("Desktop", format!("unavailable: {error}"))),
            }
        }
        let names = |list: Vec<Offered>| {
            list.iter().map(|o| o.extra.name.as_str()).collect::<Vec<_>>().join(", ")
        };
        let ticked = names(self.ticked(catalog, family));
        rows.push(("Extras", if ticked.is_empty() { "None".into() } else { ticked }));
        let built_in =
            names(catalog.extras.on(&distro.id).filter(|o| o.provision.built_in).collect());
        if !built_in.is_empty() {
            rows.push(("Built in", built_in));
        }
        rows.push((
            "Release",
            format!("{} · resolved before install", capitalize(&distro.channel.to_string())),
        ));
        rows.push(("Boot", "Limine".into()));
        let secure_boot = if self.secure_boot { "On · this machine's own keys" } else { "Off" };
        rows.push(("Secure Boot", secure_boot.into()));
        rows
    }

    /// One line on how the chosen shell (or compositor) arrives.
    pub fn arrival(&self, catalog: &Catalog, family: Family) -> String {
        let distro = &self.distro(catalog, family).id;
        let Some(choice) = self.desktop_choice(catalog, family) else { return String::new() };
        match catalog.desktops.resolve(distro, &choice) {
            Ok((Some(compositor), None)) => {
                format!("Only the compositor is installed: {}.", compositor.badge().to_lowercase())
            }
            Ok((_, Some(Source::Official { .. }))) => {
                "Installed with the system from official packages.".into()
            }
            Ok((_, Some(source @ Source::FirstLogin { .. }))) => {
                format!("Set up as you on first login: {source}")
            }
            Ok((_, Some(source))) => format!("From {source} · named in review before you approve."),
            Ok((None, _)) => "A console login, nothing extra.".into(),
            Err(error) => error.to_string(),
        }
    }
}

/// What composing offers on a distribution, in a line: "Hyprland, niri, MangoWC ·
/// 12 shells, such as Noctalia, Caelestia, and Dux Shell".
pub fn compose_teaser(catalog: &Catalog, distro: &Distro) -> String {
    let desktops = &catalog.desktops;
    let compositors: Vec<_> = desktops
        .compositors(&distro.id)
        .filter(|o| o.available())
        .map(|o| o.item.name.as_str())
        .collect();
    let shells: Vec<_> = desktops
        .shells
        .iter()
        .filter(|s| s.on.contains_key(&distro.id))
        .map(|s| s.name.as_str())
        .collect();
    let examples = match shells.as_slice() {
        [a, b, c, ..] => format!(", such as {a}, {b}, and {c}"),
        _ => String::new(),
    };
    format!("{} · {} shells{examples}", compositors.join(", "), shells.len())
}

/// The kind of device, short enough for a card: "Laptop", or "Unknown device"
/// (the reason goes on a line of its own where there is room).
pub fn device_name(hw: &borea_hardware::HardwareSnapshot) -> String {
    match &hw.device_class {
        borea_core::Fact::Known(class) => capitalize(&class.to_string()),
        borea_core::Fact::Unknown { .. } => "Unknown device".into(),
    }
}

pub fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog::bundled().unwrap()
    }

    /// Arch Linux with its Minimal edition chosen, as a click on it would.
    fn on_arch_minimal(catalog: &Catalog) -> Selection {
        let mut s = Selection::default();
        let arch = s.distro(catalog, Family::Arch);
        let minimal = arch.editions.iter().position(|e| e.composable).unwrap();
        s.set_edition(Family::Arch, minimal);
        s.ensure_defaults(catalog, Family::Arch);
        s
    }

    #[test]
    fn cards_start_on_a_ready_made_desktop() {
        let catalog = catalog();
        let s = Selection::default();
        for family in Family::ALL {
            assert!(!s.composable(&catalog, family), "{family:?} starts on Minimal");
            assert!(s.path_complete(&catalog, family));
        }
        assert_eq!(s.title(&catalog, Family::Arch), "Arch Linux · KDE Plasma");
    }

    #[test]
    fn minimal_starts_unconfirmed_with_suggested_defaults() {
        let catalog = catalog();
        let s = on_arch_minimal(&catalog);
        assert!(s.composable(&catalog, Family::Arch));
        assert!(!s.path_complete(&catalog, Family::Arch), "no preview until confirmed");
        assert_eq!(s.title(&catalog, Family::Arch), "Arch Linux · Minimal · Hyprland + Noctalia");
    }

    #[test]
    fn every_path_completes() {
        let catalog = catalog();
        let mut s = on_arch_minimal(&catalog);
        s.confirm(Family::Arch);
        assert!(s.path_complete(&catalog, Family::Arch));

        s.set_compositor(&catalog, Family::Arch, "niri");
        assert!(!s.path_complete(&catalog, Family::Arch), "a change withdraws the preview");
        s.set_shell(Family::Arch, ShellPick::NoShell);
        s.confirm(Family::Arch);
        assert_eq!(s.title(&catalog, Family::Arch), "Arch Linux · Minimal · niri");
        assert!(
            s.rows(&catalog, Family::Arch).contains(&("Shell", "None · compositor only".into()))
        );

        s.set_mode(Family::Arch, DesktopMode::Bare);
        assert!(s.path_complete(&catalog, Family::Arch));
        assert_eq!(s.title(&catalog, Family::Arch), "Arch Linux · Minimal · no desktop");
    }

    #[test]
    fn desktop_editions_complete_immediately() {
        let catalog = catalog();
        let s = Selection::default();
        assert!(s.path_complete(&catalog, Family::Fedora));
        assert_eq!(s.title(&catalog, Family::Fedora), "Fedora Linux · GNOME · Workstation");
    }

    #[test]
    fn cachyos_minimal_leads_with_its_own_settings_and_extras_are_choices() {
        let catalog = catalog();
        let mut s = Selection::default();
        s.set_distro(Family::Arch, 1);
        let minimal =
            s.distro(&catalog, Family::Arch).editions.iter().position(|e| e.composable).unwrap();
        s.set_edition(Family::Arch, minimal);
        s.ensure_defaults(&catalog, Family::Arch);
        s.set_compositor(&catalog, Family::Arch, "mango");
        s.confirm(Family::Arch);
        assert_eq!(
            s.title(&catalog, Family::Arch),
            "CachyOS · Minimal · MangoWC + Noctalia · CachyOS"
        );
        assert!(!s.extra(&catalog, Family::Arch, "gaming"), "unticked until chosen");
        assert!(s.rows(&catalog, Family::Arch).contains(&("Extras", "None".into())));

        s.set_extra(Family::Arch, "gaming", true);
        s.set_extra(Family::Arch, "codecs", true);
        assert!(
            s.rows(&catalog, Family::Arch).contains(&("Extras", "Gaming, Media codecs".into()))
        );
        s.set_edition(Family::Arch, 0);
        assert!(s.extra(&catalog, Family::Arch, "gaming"), "kept across editions");
        s.set_distro(Family::Arch, 0);
        assert!(s.ticked(&catalog, Family::Arch).is_empty(), "a new distribution starts fresh");
    }

    #[test]
    fn tumbleweed_ticks_gaming_and_bazzite_has_extras_built_in() {
        let catalog = catalog();
        let mut s = Selection::default();
        assert!(s.extra(&catalog, Family::Opensuse, "gaming"));
        s.set_extra(Family::Opensuse, "gaming", false);
        assert!(!s.extra(&catalog, Family::Opensuse, "gaming"));

        s.set_distro(Family::Fedora, 1);
        let rows = s.rows(&catalog, Family::Fedora);
        assert!(rows.contains(&("Built in", "Gaming, Media codecs, Flatpak".into())));
        assert!(rows.contains(&("Extras", "None".into())), "nothing left to tick on Bazzite");
        assert!(!s.extra(&catalog, Family::Fedora, "gaming"), "built in is not a checkbox");
    }

    #[test]
    fn every_family_can_compose_from_any_card() {
        let catalog = catalog();
        let mut s = Selection::default();
        // Fedora starts on GNOME · Workstation: Minimal is a click away on the same distribution.
        let target = s.compose_target(&catalog, Family::Fedora).unwrap();
        assert_eq!(target.0, 0);
        s.compose_on(Family::Fedora, target);
        assert!(s.composable(&catalog, Family::Fedora));
        assert!(s.compose_target(&catalog, Family::Fedora).is_none(), "already composing");

        // Bazzite has no Minimal: the offer moves to Fedora Linux.
        s.set_distro(Family::Fedora, 1);
        assert_eq!(s.compose_target(&catalog, Family::Fedora).map(|t| t.0), Some(0));
        let teaser = compose_teaser(&catalog, catalog.get("arch").unwrap());
        assert!(teaser.starts_with("Hyprland, niri, MangoWC · "), "{teaser}");
        assert!(teaser.contains(" shells, such as "), "{teaser}");
    }

    #[test]
    fn switching_distribution_resets_the_card() {
        let catalog = catalog();
        let mut s = Selection::default();
        s.ensure_defaults(&catalog, Family::Arch);
        s.confirm(Family::Arch);
        s.set_distro(Family::Arch, 2);
        assert!(!s.composable(&catalog, Family::Arch), "Omarchy ships its own desktop");
        assert!(s.choice(Family::Arch).compositor.is_none());
    }
}
