//! The Windpad catalog: family cards, distribution dropdowns, edition dropdowns, and
//! the compose-your-desktop path for editions shipped without a desktop.
//!
//! The catalog describes *what* can be offered. It deliberately says nothing about
//! the host Borea is running on, and nothing about *how* a target is obtained;
//! that belongs to distribution adapters (`borea-distro`).

pub mod desktop;
pub mod extras;

use std::collections::BTreeSet;

use borea_core::{DeviceClass, DistroId, Family, ReleaseChannel, SupportStatus, X86Level};
use serde::Deserialize;
use thiserror::Error;

pub use desktop::{ChoiceError, Compositor, DesktopChoice, Desktops, Offer, Shell, Source};
pub use extras::{Extra, Extras, Offered, Provision};

/// Development catalog bundled with this build. Unsigned research data.
pub const BUNDLED: &str = include_str!("../data/catalog.toml");

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct Catalog {
    /// Date the entries were last reviewed against upstream sources.
    pub reviewed: String,
    /// Whether this catalog arrived through verified signed metadata.
    pub signed: bool,
    pub distros: Vec<Distro>,
    pub desktops: Desktops,
    pub extras: Extras,
}

/// `catalog.toml` as written; combined with `shells.toml` into a [`Catalog`].
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Listing {
    schema: u32,
    reviewed: String,
    signed: bool,
    #[serde(rename = "distro")]
    distros: Vec<Distro>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Distro {
    pub id: DistroId,
    pub name: String,
    pub family: Family,
    pub channel: ReleaseChannel,
    pub status: SupportStatus,
    pub summary: String,
    /// Official upstream page the entry was reviewed against.
    pub source: String,
    pub editions: Vec<Edition>,
    /// Image variants offered for every edition, e.g. Standard / NVIDIA.
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(default)]
    pub cpu_baseline: Option<X86Level>,
    /// False while the full upstream edition matrix is not yet represented.
    #[serde(default = "default_true")]
    pub matrix_complete: bool,
}

impl Distro {
    pub fn edition(&self, id: &str) -> Option<&Edition> {
        self.editions.iter().find(|e| e.id == id)
    }

    /// The edition that ships without a desktop, if this distribution has one.
    pub fn composable(&self) -> Option<&Edition> {
        self.editions.iter().find(|e| e.composable)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edition {
    pub id: String,
    pub name: String,
    /// The device class this edition is designed for, if it is device-specific.
    #[serde(default)]
    pub device: Option<DeviceClass>,
    /// No preinstalled desktop environment or shell: the user composes one.
    #[serde(default)]
    pub composable: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("catalog could not be parsed: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("catalog schema {found} is not supported (expected {SCHEMA_VERSION})")]
    Schema { found: u32 },
    #[error("distribution id `{0}` appears more than once")]
    DuplicateDistro(DistroId),
    #[error("`{distro}` lists edition `{edition}` more than once")]
    DuplicateEdition { distro: DistroId, edition: String },
    #[error("`{0}` has no editions; Windpad needs at least one to offer")]
    NoEditions(DistroId),
    #[error("family card {0} has no distributions")]
    EmptyFamily(Family),
    #[error("`{0}` is marked supported, but an unsigned catalog cannot certify anything")]
    UnsignedSupport(DistroId),
    #[error("{what} `{id}` is offered on `{distro}`, which has no composable edition")]
    NotComposable { what: &'static str, id: String, distro: DistroId },
    #[error("shell `{shell}` names unknown compositor `{compositor}`")]
    UnknownCompositor { shell: String, compositor: String },
    #[error("`{distro}` offers {compositor} but no shell for it")]
    NoShell { distro: DistroId, compositor: String },
    #[error("`{0}` has a composable edition but no compositor to compose with")]
    NoCompositor(DistroId),
    #[error("shell `{shell}` is made for `{distro}` but not offered there")]
    MadeForUnoffered { shell: String, distro: DistroId },
    #[error("extra `{extra}` on `{distro}`: {problem}")]
    Extra { extra: String, distro: DistroId, problem: &'static str },
}

impl Catalog {
    pub fn parse(catalog: &str, shells: &str, extras: &str) -> Result<Self, CatalogError> {
        let listing: Listing = toml::from_str(catalog)?;
        let desktops: Desktops = toml::from_str(shells)?;
        let extras: Extras = toml::from_str(extras)?;
        if let Some(found) = [listing.schema, desktops.schema, extras.schema]
            .into_iter()
            .find(|s| *s != SCHEMA_VERSION)
        {
            return Err(CatalogError::Schema { found });
        }
        let catalog = Catalog {
            reviewed: listing.reviewed,
            signed: listing.signed,
            distros: listing.distros,
            desktops,
            extras,
        };
        catalog.validate()?;
        Ok(catalog)
    }

    pub fn bundled() -> Result<Self, CatalogError> {
        Self::parse(BUNDLED, desktop::BUNDLED, extras::BUNDLED)
    }

    fn validate(&self) -> Result<(), CatalogError> {
        let mut ids = BTreeSet::new();
        for distro in &self.distros {
            if !ids.insert(&distro.id) {
                return Err(CatalogError::DuplicateDistro(distro.id.clone()));
            }
            if distro.editions.is_empty() {
                return Err(CatalogError::NoEditions(distro.id.clone()));
            }
            let mut editions = BTreeSet::new();
            if let Some(edition) = distro.editions.iter().find(|e| !editions.insert(&e.id)) {
                return Err(CatalogError::DuplicateEdition {
                    distro: distro.id.clone(),
                    edition: edition.id.clone(),
                });
            }
            if !self.signed && distro.status == SupportStatus::Supported {
                return Err(CatalogError::UnsignedSupport(distro.id.clone()));
            }
        }
        if let Some(family) = Family::ALL.into_iter().find(|f| self.family(*f).next().is_none()) {
            return Err(CatalogError::EmptyFamily(family));
        }
        for extra in &self.extras.extras {
            for (distro, provision) in &extra.on {
                let problem = match self.get(distro.as_str()) {
                    None => Some("no such distribution"),
                    Some(_) => Extras::problem(provision),
                };
                if let Some(problem) = problem {
                    return Err(CatalogError::Extra {
                        extra: extra.id.clone(),
                        distro: distro.clone(),
                        problem,
                    });
                }
            }
        }
        self.validate_desktops()
    }

    /// The compose path must never lead somewhere empty: every offered compositor
    /// has at least one shell, and only composable distributions are offered anything.
    fn validate_desktops(&self) -> Result<(), CatalogError> {
        let desktops = &self.desktops;
        let composable =
            |id: &DistroId| self.get(id.as_str()).is_some_and(|d| d.composable().is_some());

        let offered = desktops
            .compositors
            .iter()
            .map(|c| ("compositor", &c.id, &c.on))
            .chain(desktops.shells.iter().map(|s| ("shell", &s.id, &s.on)));
        for (what, id, on) in offered {
            if let Some(distro) = on.keys().find(|d| !composable(d)) {
                return Err(CatalogError::NotComposable {
                    what,
                    id: id.clone(),
                    distro: distro.clone(),
                });
            }
        }
        for shell in &desktops.shells {
            if let Some(distro) = shell.made_for.iter().find(|d| !shell.on.contains_key(*d)) {
                return Err(CatalogError::MadeForUnoffered {
                    shell: shell.id.clone(),
                    distro: distro.clone(),
                });
            }
            let known = |c: &&String| desktops.compositors.iter().any(|k| &k.id == *c);
            if let Some(unknown) = shell.compositors.iter().find(|c| !known(c)) {
                return Err(CatalogError::UnknownCompositor {
                    shell: shell.id.clone(),
                    compositor: unknown.clone(),
                });
            }
        }
        for distro in self.distros.iter().filter(|d| d.composable().is_some()) {
            let mut available =
                desktops.compositors(&distro.id).filter(|o| o.available()).peekable();
            if available.peek().is_none() {
                return Err(CatalogError::NoCompositor(distro.id.clone()));
            }
            if let Some(empty) =
                available.find(|c| desktops.shells(&distro.id, &c.item.id).next().is_none())
            {
                return Err(CatalogError::NoShell {
                    distro: distro.id.clone(),
                    compositor: empty.item.name.clone(),
                });
            }
        }
        Ok(())
    }

    /// Distributions for one family card, in catalog order (the dropdown order).
    pub fn family(&self, family: Family) -> impl Iterator<Item = &Distro> {
        self.distros.iter().filter(move |d| d.family == family)
    }

    pub fn get(&self, id: &str) -> Option<&Distro> {
        self.distros.iter().find(|d| d.id.as_str() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(catalog: &str) -> Result<Catalog, CatalogError> {
        Catalog::parse(catalog, desktop::BUNDLED, extras::BUNDLED)
    }

    fn id(s: &str) -> DistroId {
        DistroId(s.into())
    }

    #[test]
    fn bundled_catalog_is_valid() {
        let catalog = Catalog::bundled().unwrap();
        assert!(!catalog.signed);
    }

    #[test]
    fn bundled_catalog_has_every_requested_distribution() {
        let catalog = Catalog::bundled().unwrap();
        let expect = [
            (Family::Arch, &["arch", "cachyos", "omarchy"][..]),
            (Family::Fedora, &["fedora", "bazzite", "nobara"]),
            (Family::Debian, &["debian", "pikaos"]),
            (Family::Opensuse, &["tumbleweed"]),
        ];
        for (family, ids) in expect {
            let found: Vec<_> = catalog.family(family).map(|d| d.id.as_str()).collect();
            assert_eq!(found, ids, "{family} card");
        }
    }

    /// The menus read alike on every card: one name for the desktop-free edition,
    /// desktop first, upstream's label after " · ", and the same desktop names.
    #[test]
    fn edition_names_follow_one_convention() {
        let catalog = Catalog::bundled().unwrap();
        let desktops = ["KDE Plasma", "GNOME", "Xfce", "Hyprland", "niri", "COSMIC"];
        for distro in &catalog.distros {
            for edition in &distro.editions {
                let name = edition.name.as_str();
                if edition.composable {
                    assert_eq!(name, "Minimal", "{}", distro.id);
                } else {
                    let desktop = name.split(" · ").next().unwrap();
                    assert!(desktops.contains(&desktop), "{}: `{name}`", distro.id);
                }
            }
        }
    }

    #[test]
    fn extras_cover_every_distribution() {
        let catalog = Catalog::bundled().unwrap();
        for distro in &catalog.distros {
            let ids: Vec<_> = catalog.extras.on(&distro.id).map(|o| o.extra.id.as_str()).collect();
            assert_eq!(ids.first(), Some(&"gaming"), "{}: Gaming everywhere, first", distro.id);
        }
        let ticked: Vec<_> = catalog
            .extras
            .extras
            .iter()
            .flat_map(|e| e.on.iter().filter(|(_, p)| p.default).map(|(d, _)| (&*e.id, d.as_str())))
            .collect();
        assert_eq!(ticked, [("gaming", "tumbleweed")], "only Tumbleweed ticks anything");
        let cachyos = catalog.extras.on(&id("cachyos")).next().unwrap();
        assert_eq!(
            cachyos.provision.packages,
            ["cachyos-gaming-meta", "cachyos-gaming-applications"]
        );
        assert!(cachyos.describe().starts_with("Proton-CachyOS"));
        let fedora = catalog.extras.on(&id("fedora")).next().unwrap();
        assert!(fedora.describe().ends_with("· enables RPM Fusion nonfree, for Steam"));
    }

    #[test]
    fn broken_extras_are_refused() {
        let extras = |on: &str| {
            format!(
                "schema = 1\nreviewed = \"x\"\n[[extra]]\nid = \"e\"\nname = \"E\"\nsummary = \"s\"\n{on}\n"
            )
        };
        for (on, problem) in [
            ("on.nowhere = { packages = [\"x\"] }", "no such distribution"),
            ("on.arch = { }", "nothing to install"),
            ("on.arch = { built_in = true, packages = [\"x\"] }", "yet built in"),
        ] {
            let error = Catalog::parse(BUNDLED, desktop::BUNDLED, &extras(on)).unwrap_err();
            assert!(error.to_string().contains(problem), "{on}: {error}");
        }
    }

    #[test]
    fn requested_editions_are_present() {
        let catalog = Catalog::bundled().unwrap();
        let editions = |id| -> Vec<String> {
            catalog.get(id).unwrap().editions.iter().map(|e| e.name.clone()).collect()
        };
        assert_eq!(editions("cachyos"), ["KDE Plasma", "KDE Plasma · Handheld", "Minimal"]);
        assert_eq!(
            editions("nobara"),
            [
                "KDE Plasma · Official",
                "GNOME",
                "KDE Plasma",
                "KDE Plasma · Steam HTPC",
                "KDE Plasma · Steam Handheld"
            ]
        );
        assert_eq!(editions("pikaos"), ["GNOME", "KDE Plasma", "Hyprland", "niri", "COSMIC"]);
        assert_eq!(catalog.get("pikaos").unwrap().variants, ["Standard", "NVIDIA"]);
        assert!(!catalog.get("bazzite").unwrap().matrix_complete);
    }

    #[test]
    fn only_desktop_free_editions_are_composable() {
        let catalog = Catalog::bundled().unwrap();
        let composable: Vec<_> = catalog
            .distros
            .iter()
            .filter(|d| d.composable().is_some())
            .map(|d| d.id.as_str())
            .collect();
        assert_eq!(composable, ["arch", "cachyos", "fedora", "debian", "tumbleweed"]);
        // CachyOS composes only on Minimal; its KDE Plasma editions keep theirs.
        assert_eq!(catalog.get("cachyos").unwrap().composable().unwrap().name, "Minimal");
        // These derivatives ship only editions with their own desktop or shell.
        for preinstalled in ["omarchy", "bazzite", "nobara", "pikaos"] {
            assert!(catalog.get(preinstalled).unwrap().composable().is_none(), "{preinstalled}");
        }
    }

    #[test]
    fn compose_path_follows_what_each_distribution_packages() {
        let d = Catalog::bundled().unwrap().desktops;
        let available = |distro: &str| -> Vec<String> {
            let distro = id(distro);
            d.compositors(&distro).filter(|o| o.available()).map(|o| o.item.id.clone()).collect()
        };
        assert_eq!(available("arch"), ["hyprland", "niri", "mango"]);
        assert_eq!(available("cachyos"), ["hyprland", "niri", "mango"]);
        assert_eq!(available("fedora"), ["hyprland", "niri", "mango"]);
        assert_eq!(available("debian"), ["hyprland", "mango"]);
        assert_eq!(available("tumbleweed"), ["hyprland", "niri"]);

        let shells = |distro: &str, compositor: &str| -> Vec<String> {
            let distro = id(distro);
            d.shells(&distro, compositor).map(|o| o.item.id.clone()).collect()
        };
        assert_eq!(
            shells("arch", "niri"),
            ["noctalia", "dux", "serpantinum", "ambxst", "inir", "ashell", "waybar"]
        );
        assert_eq!(shells("debian", "hyprland"), ["noctalia", "dux", "waybar"]);
        assert_eq!(shells("debian", "mango"), ["noctalia", "dux", "mangobutter", "waybar"]);
        assert!(!shells("arch", "mango").iter().any(|s| s == "mangobutter"), "Debian-only");
        // CachyOS gets every Arch shell plus its own tuned settings.
        assert_eq!(shells("cachyos", "mango").len(), shells("arch", "mango").len() + 2);
        assert!(!shells("arch", "hyprland").iter().any(|s| s.starts_with("cachyos-")));
        assert!(!shells("arch", "mango").iter().any(|s| s == "caelestia"));
    }

    #[test]
    fn suggestion_prefers_official_packages() {
        let d = Catalog::bundled().unwrap().desktops;
        // illogical-impulse is most starred but installs at first login; Noctalia is official.
        assert_eq!(d.suggested(&id("arch"), "hyprland").unwrap().id, "noctalia");
        // On Debian, Noctalia needs a third-party repo, so the official Waybar trio leads.
        assert_eq!(d.suggested(&id("debian"), "hyprland").unwrap().id, "waybar");
        let debian = id("debian");
        let ranked: Vec<_> = d.ranked(&debian, "hyprland").map(|o| o.item.id.as_str()).collect();
        assert_eq!(ranked, ["waybar", "noctalia", "dux"], "the suggestion leads");
        // A setup made for Debian leads there, ahead of the official fallback.
        assert_eq!(d.suggested(&debian, "mango").unwrap().id, "mangobutter");
        // CachyOS's own tuned settings lead on CachyOS.
        let cachy = id("cachyos");
        assert_eq!(d.suggested(&cachy, "hyprland").unwrap().id, "cachyos-noctalia-hyprland");
        assert_eq!(d.suggested(&cachy, "mango").unwrap().id, "cachyos-noctalia-mango");
    }

    #[test]
    fn every_step_of_the_path_is_a_complete_answer() {
        let d = Catalog::bundled().unwrap().desktops;
        let debian = id("debian");
        let compositor = |c: &str| DesktopChoice::Compositor { compositor: c.into() };
        let shell =
            |c: &str, s: &str| DesktopChoice::Shell { compositor: c.into(), shell: s.into() };

        // Continue without a desktop, or without a shell: both are valid choices.
        assert_eq!(d.resolve(&debian, &DesktopChoice::None), Ok((None, None)));
        let (via, none) = d.resolve(&debian, &compositor("hyprland")).unwrap();
        assert_eq!(via.unwrap().badge(), "Official");
        assert!(none.is_none());

        let (_, via) = d.resolve(&debian, &shell("mango", "mangobutter")).unwrap();
        assert_eq!(via.unwrap().badge(), "First-login setup");
        assert!(matches!(
            d.resolve(&debian, &compositor("niri")),
            Err(ChoiceError::CompositorUnavailable { .. })
        ));
        assert!(matches!(
            d.resolve(&debian, &shell("hyprland", "inir")),
            Err(ChoiceError::Unsupported { .. })
        ));
        assert!(matches!(
            d.resolve(&id("arch"), &shell("mango", "mangobutter")),
            Err(ChoiceError::ShellUnavailable { .. })
        ));
    }

    #[test]
    fn shells_cannot_target_preinstalled_desktops() {
        let shells = desktop::BUNDLED.replacen(
            r#"on.arch = { via = "aur", packages = ["caelestia-shell"] }"#,
            r#"on.omarchy = { via = "aur", packages = ["caelestia-shell"] }"#,
            1,
        );
        assert!(matches!(
            Catalog::parse(BUNDLED, &shells, extras::BUNDLED),
            Err(CatalogError::NotComposable { distro, .. }) if distro.as_str() == "omarchy"
        ));
    }

    #[test]
    fn unknown_compositors_are_rejected() {
        let shells = desktop::BUNDLED.replacen(
            r#"compositors = ["hyprland"]"#,
            r#"compositors = ["kwin"]"#,
            1,
        );
        assert!(matches!(
            Catalog::parse(BUNDLED, &shells, extras::BUNDLED),
            Err(CatalogError::UnknownCompositor { .. })
        ));
    }

    #[test]
    fn unsigned_catalog_cannot_certify() {
        let text = BUNDLED.replacen(r#"status = "planned""#, r#"status = "supported""#, 1);
        assert!(matches!(parse(&text), Err(CatalogError::UnsignedSupport(_))));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let text = BUNDLED.replacen(r#"id = "cachyos""#, r#"id = "arch""#, 1);
        assert!(matches!(parse(&text), Err(CatalogError::DuplicateDistro(_))));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let text = BUNDLED.replacen("schema = 1", "schema = 1\nlatest = \"43\"", 1);
        assert!(matches!(parse(&text), Err(CatalogError::Parse(_))));
    }
}
