#![forbid(unsafe_code)]
//! Turning picks into an install plan. Every picked app gets exactly one source.

use std::collections::{BTreeMap, BTreeSet};

use crate::catalog::{APPS, App, app};
use crate::source::{Distro, Source, Sources};

/// The "install from" switch, like WinUtil's winget / Chocolatey choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Prefer {
    /// Distro packages, then AUR on Arch, Flathub, and an upstream AppImage.
    Native,
    /// Flathub first, then distro packages, then an upstream AppImage.
    Flatpak,
    /// Default: AUR on Arch first, then native packages, Flathub, and an upstream AppImage.
    #[default]
    Aur,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Installs from this source as things stand.
    From { source: Source, package: &'static str },
    /// Available, but only from a source that is switched off.
    NeedsSource { source: Source, package: &'static str },
    /// Not offered for this distro at all.
    Unavailable,
}

/// Every place this app could come from on this distro, in preference order.
pub fn candidates(a: &App, distro: Distro, prefer: Prefer) -> Vec<(Source, &'static str)> {
    let native: Vec<(Source, &'static str)> = a
        .native
        .iter()
        .filter(|n| n.distro == distro)
        .map(|n| (Source::Repo(n.repo), n.pkg))
        .collect();
    let aur = a.aur.filter(|_| distro == Distro::Arch).map(|p| (Source::Aur, p));
    let flat = a.flatpak.map(|id| (Source::Flathub, id));
    // Last resort everywhere: packaged sources update with the system, an AppImage doesn't.
    let appimage = a.appimage.map(|r| (Source::AppImage, r));

    let mut out = Vec::new();
    match prefer {
        Prefer::Native => {
            out.extend(native);
            out.extend(aur);
            out.extend(flat);
        }
        Prefer::Aur => {
            out.extend(aur);
            out.extend(native);
            out.extend(flat);
        }
        Prefer::Flatpak => {
            out.extend(flat);
            out.extend(native);
            out.extend(aur);
        }
    }
    out.extend(appimage);
    out
}

pub fn resolve(a: &App, sources: &Sources, prefer: Prefer) -> Resolution {
    let c = candidates(a, sources.distro(), prefer);
    if let Some(&(source, package)) = c.iter().find(|(s, _)| sources.is_on(*s)) {
        return Resolution::From { source, package };
    }
    match c.first() {
        Some(&(source, package)) => Resolution::NeedsSource { source, package },
        None => Resolution::Unavailable,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Selection {
    picked: BTreeSet<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Pick {
    Added,
    /// Added, and this source was switched on so it could install.
    AddedAndEnabled(Source),
    Removed,
    Unavailable,
    UnknownApp,
}

impl Selection {
    pub fn with(ids: &[&str]) -> Self {
        let mut s = Selection::default();
        for id in ids {
            if let Some(a) = app(id) {
                s.picked.insert(a.id);
            }
        }
        s
    }

    pub fn contains(&self, id: &str) -> bool {
        self.picked.contains(id)
    }

    pub fn len(&self) -> usize {
        self.picked.len()
    }

    pub fn is_empty(&self) -> bool {
        self.picked.is_empty()
    }

    pub fn clear(&mut self) {
        self.picked.clear();
    }

    /// Tick or untick one app. Ticking an app whose only source is off switches that source on,
    /// so a beginner never ends up with a ticked box that silently installs nothing.
    pub fn toggle(&mut self, id: &str, sources: &mut Sources, prefer: Prefer) -> Pick {
        let Some(a) = app(id) else {
            return Pick::UnknownApp;
        };
        if self.picked.remove(a.id) {
            return Pick::Removed;
        }
        match resolve(a, sources, prefer) {
            Resolution::From { .. } => {
                self.picked.insert(a.id);
                Pick::Added
            }
            Resolution::NeedsSource { source, .. } => {
                if sources.set(source, true).is_err() {
                    return Pick::Unavailable;
                }
                self.picked.insert(a.id);
                Pick::AddedAndEnabled(source)
            }
            Resolution::Unavailable => Pick::Unavailable,
        }
    }

    /// Apps that can't come from the chosen distro at all are dropped when switching distro.
    pub fn retain_available(&mut self, distro: Distro) {
        self.picked.retain(|id| {
            app(id).is_some_and(|a| !candidates(a, distro, Prefer::Native).is_empty())
        });
    }
}

#[derive(Debug, Default)]
pub struct Plan {
    /// Packages grouped by where they come from, in install order (repos, AUR, Flathub, AppImage).
    pub batches: BTreeMap<Source, Vec<&'static str>>,
    /// Picked apps whose source got switched off afterwards.
    pub stranded: Vec<&'static str>,
    pub size_mb: u32,
}

impl Plan {
    pub fn count(&self) -> usize {
        self.batches.values().map(Vec::len).sum()
    }
}

pub fn plan(selection: &Selection, sources: &Sources, prefer: Prefer) -> Plan {
    let mut p = Plan::default();
    for a in APPS.iter().filter(|a| selection.contains(a.id)) {
        match resolve(a, sources, prefer) {
            Resolution::From { source, package } => {
                p.batches.entry(source).or_default().push(package);
                p.size_mb += a.mb;
            }
            _ => p.stranded.push(a.id),
        }
    }
    p
}
