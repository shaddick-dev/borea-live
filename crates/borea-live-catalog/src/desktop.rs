//! Compose your desktop: compositors and shells for editions shipped without one.
//!
//! The path is deliberately linear: **edition → compositor → shell**. Each step only
//! offers what the chosen distribution can actually install, and says how it will
//! arrive (official package, disclosed third-party repository, or a first-login
//! installer run as the user).

use std::collections::BTreeMap;
use std::fmt;

use borea_live_core::DistroId;
use serde::Deserialize;

/// Compositor and shell data bundled with this build. Unsigned research data.
pub const BUNDLED: &str = include_str!("../data/shells.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Desktops {
    pub schema: u32,
    pub reviewed: String,
    #[serde(rename = "compositor")]
    pub compositors: Vec<Compositor>,
    #[serde(rename = "shell")]
    pub shells: Vec<Shell>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compositor {
    pub id: String,
    pub name: String,
    /// One line on how it feels to use.
    pub style: String,
    pub license: String,
    pub source: String,
    /// How each distribution provides it. Absent means unavailable there.
    pub on: BTreeMap<DistroId, Source>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shell {
    pub id: String,
    pub name: String,
    pub by: String,
    pub framework: String,
    pub license: String,
    pub summary: String,
    pub source: String,
    /// Compositor ids this shell supports.
    pub compositors: Vec<String>,
    /// Distributions this shell was built specifically for, such as a curated
    /// community setup. Such a shell leads the suggestions there.
    #[serde(default)]
    pub made_for: Vec<DistroId>,
    pub on: BTreeMap<DistroId, Source>,
}

impl Shell {
    pub fn supports(&self, compositor: &str) -> bool {
        self.compositors.iter().any(|c| c == compositor)
    }

    pub fn made_for(&self, distro: &DistroId) -> bool {
        self.made_for.contains(distro)
    }
}

/// How a compositor or shell reaches the installed system.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "via", rename_all = "kebab-case")]
pub enum Source {
    /// The distribution's own repositories, installed with the target.
    Official {
        packages: Vec<String>,
        #[serde(default)]
        suite: Option<String>,
    },
    /// A named external repository, disclosed in review before approval.
    ThirdParty { packages: Vec<String>, repository: String },
    /// Arch User Repository, built as the user at first login, never as root.
    Aur { packages: Vec<String> },
    /// The project's own installer, run as the user at first login.
    FirstLogin { installer: String },
}

impl Source {
    /// Lower is more integrated with the distribution.
    pub fn rank(&self) -> u8 {
        match self {
            Source::Official { .. } => 0,
            Source::ThirdParty { .. } => 1,
            Source::Aur { .. } => 2,
            Source::FirstLogin { .. } => 3,
        }
    }

    /// Short badge text for Windpad cards.
    pub fn badge(&self) -> &'static str {
        match self {
            Source::Official { .. } => "Official",
            Source::ThirdParty { .. } => "Third-party repo",
            Source::Aur { .. } => "AUR",
            Source::FirstLogin { .. } => "First-login setup",
        }
    }

    /// Installed during the privileged install, rather than as the user afterwards.
    pub fn installs_with_target(&self) -> bool {
        matches!(self, Source::Official { .. } | Source::ThirdParty { .. })
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Official { packages, suite: None } => write!(f, "{}", packages.join(" ")),
            Source::Official { packages, suite: Some(suite) } => {
                write!(f, "{} from {suite}", packages.join(" "))
            }
            Source::ThirdParty { packages, repository } => {
                write!(f, "{} from {repository}", packages.join(" "))
            }
            Source::Aur { packages } => f.write_str(&packages.join(" ")),
            Source::FirstLogin { installer } => write!(f, "{installer}"),
        }
    }
}

/// What the user chose for a composable edition. Every level is a complete answer:
/// the user can stop at any step and continue the installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopChoice {
    /// Minimal exactly as shipped: no compositor, no shell, a console login.
    None,
    /// A compositor with its own defaults and no shell on top.
    Compositor { compositor: String },
    /// A compositor and a shell to live on it.
    Shell { compositor: String, shell: String },
}

impl DesktopChoice {
    pub fn compositor(&self) -> Option<&str> {
        match self {
            DesktopChoice::None => None,
            DesktopChoice::Compositor { compositor } | DesktopChoice::Shell { compositor, .. } => {
                Some(compositor)
            }
        }
    }

    pub fn shell(&self) -> Option<&str> {
        match self {
            DesktopChoice::Shell { shell, .. } => Some(shell),
            _ => None,
        }
    }
}

/// Why a [`DesktopChoice`] cannot be installed on a distribution.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChoiceError {
    #[error("no compositor `{0}`")]
    UnknownCompositor(String),
    #[error("{compositor} is not packaged for `{distro}`")]
    CompositorUnavailable { compositor: String, distro: DistroId },
    #[error("no shell `{0}`")]
    UnknownShell(String),
    #[error("{shell} does not run on {compositor}")]
    Unsupported { shell: String, compositor: String },
    #[error("{shell} is not available for `{distro}`")]
    ShellUnavailable { shell: String, distro: DistroId },
}

/// One step of the compose path: an option, and how it arrives here (if it can).
#[derive(Debug, Clone, Copy)]
pub struct Offer<'a, T> {
    pub item: &'a T,
    pub source: Option<&'a Source>,
}

impl<T> Offer<'_, T> {
    pub fn available(&self) -> bool {
        self.source.is_some()
    }
}

impl Desktops {
    /// Every compositor, with how `distro` provides it. Unavailable ones are kept so
    /// Windpad can show them quietly disabled with a reason, rather than vanish.
    pub fn compositors<'a>(
        &'a self,
        distro: &'a DistroId,
    ) -> impl Iterator<Item = Offer<'a, Compositor>> {
        self.compositors.iter().map(move |item| Offer { item, source: item.on.get(distro) })
    }

    /// Shells installable on `distro` for `compositor`, in research order.
    pub fn shells<'a>(
        &'a self,
        distro: &'a DistroId,
        compositor: &'a str,
    ) -> impl Iterator<Item = Offer<'a, Shell>> {
        self.shells
            .iter()
            .filter(move |shell| shell.supports(compositor))
            .filter_map(move |item| Some(Offer { item, source: Some(item.on.get(distro)?) }))
    }

    /// Check a choice against what `distro` can actually install, returning how the
    /// compositor and shell (if any) arrive. `None` is always valid.
    pub fn resolve<'a>(
        &'a self,
        distro: &DistroId,
        choice: &DesktopChoice,
    ) -> Result<(Option<&'a Source>, Option<&'a Source>), ChoiceError> {
        let Some(id) = choice.compositor() else { return Ok((None, None)) };
        let compositor = self
            .compositors
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| ChoiceError::UnknownCompositor(id.into()))?;
        let via = compositor.on.get(distro).ok_or_else(|| ChoiceError::CompositorUnavailable {
            compositor: compositor.name.clone(),
            distro: distro.clone(),
        })?;
        let Some(id) = choice.shell() else { return Ok((Some(via), None)) };
        let shell = self
            .shells
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| ChoiceError::UnknownShell(id.into()))?;
        if !shell.supports(&compositor.id) {
            return Err(ChoiceError::Unsupported {
                shell: shell.name.clone(),
                compositor: compositor.name.clone(),
            });
        }
        let shell_via = shell.on.get(distro).ok_or_else(|| ChoiceError::ShellUnavailable {
            shell: shell.name.clone(),
            distro: distro.clone(),
        })?;
        Ok((Some(via), Some(shell_via)))
    }

    /// Shells in the order Windpad presents them: the suggestion first, then research order.
    pub fn ranked<'a>(
        &'a self,
        distro: &'a DistroId,
        compositor: &'a str,
    ) -> impl Iterator<Item = Offer<'a, Shell>> {
        let suggested = self.suggested(distro, compositor).map(|s| &s.id);
        let is_suggested = move |o: &Offer<'a, Shell>| Some(&o.item.id) == suggested;
        let first = self.shells(distro, compositor).filter(is_suggested);
        first.chain(self.shells(distro, compositor).filter(move |o| !is_suggested(o)))
    }

    /// The shell Windpad pre-selects: one made for this distribution, else the first
    /// official package, else the first offer.
    pub fn suggested<'a>(&'a self, distro: &'a DistroId, compositor: &'a str) -> Option<&'a Shell> {
        let offers: Vec<_> = self.shells(distro, compositor).collect();
        offers
            .iter()
            .find(|o| o.item.made_for(distro))
            .or_else(|| offers.iter().find(|o| o.source.is_some_and(|s| s.rank() == 0)))
            .or(offers.first())
            .map(|o| o.item)
    }
}
