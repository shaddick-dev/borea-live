#![forbid(unsafe_code)]
//! Where software comes from: the chosen distro's own repos, the AUR (Arch only), Flathub, and
//! upstream AppImages (one self-contained file from the app's own release page, any distro).
//! A machine only ever sees the repos of the distro it is installing — never another distro's.

use std::collections::BTreeSet;
use std::fmt;

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Distro {
    Arch,
    Debian,
    Fedora,
    OpenSuse,
}

impl Distro {
    pub const ALL: [Distro; 4] = [Distro::Arch, Distro::Debian, Distro::Fedora, Distro::OpenSuse];

    pub fn name(self) -> &'static str {
        match self {
            Distro::Arch => "Arch Linux",
            Distro::Debian => "Debian",
            Distro::Fedora => "Fedora",
            Distro::OpenSuse => "openSUSE",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Distro::Arch => "arch",
            Distro::Debian => "debian",
            Distro::Fedora => "fedora",
            Distro::OpenSuse => "opensuse",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    /// Run by the distro itself.
    Official,
    /// Long-standing community repo the distro's users rely on.
    Community,
}

/// One repository of one distro.
#[derive(Debug)]
pub struct Repo {
    pub id: &'static str,
    pub distro: Distro,
    pub label: &'static str,
    pub trust: Trust,
    /// Base repos are always on and cannot be switched off.
    pub base: bool,
    /// Switched on unless the user says otherwise.
    pub default_on: bool,
    /// Only offered when the hardware needs it (e.g. openSUSE's NVIDIA repo); hidden otherwise.
    pub on_demand: bool,
    pub note: &'static str,
}

pub const REPOS: &[Repo] = &[
    Repo {
        id: "arch.core-extra",
        distro: Distro::Arch,
        label: "core & extra",
        trust: Trust::Official,
        base: true,
        default_on: true,
        on_demand: false,
        note: "the Arch repositories",
    },
    Repo {
        id: "arch.multilib",
        distro: Distro::Arch,
        label: "multilib",
        trust: Trust::Official,
        base: false,
        default_on: true,
        on_demand: false,
        note: "32-bit libraries for Steam and Wine",
    },
    Repo {
        id: "debian.main",
        distro: Distro::Debian,
        label: "main",
        trust: Trust::Official,
        base: true,
        default_on: true,
        on_demand: false,
        note: "the Debian archive",
    },
    Repo {
        id: "debian.contrib",
        distro: Distro::Debian,
        label: "contrib",
        trust: Trust::Official,
        base: false,
        default_on: true,
        on_demand: false,
        note: "free software with non-free parts",
    },
    Repo {
        id: "debian.non-free",
        distro: Distro::Debian,
        label: "non-free",
        trust: Trust::Official,
        base: false,
        default_on: true,
        on_demand: false,
        note: "NVIDIA drivers and Steam",
    },
    Repo {
        id: "debian.non-free-firmware",
        distro: Distro::Debian,
        label: "non-free-firmware",
        trust: Trust::Official,
        base: false,
        default_on: true,
        on_demand: false,
        note: "firmware for Wi-Fi, graphics and CPUs",
    },
    Repo {
        id: "fedora.fedora",
        distro: Distro::Fedora,
        label: "Fedora",
        trust: Trust::Official,
        base: true,
        default_on: true,
        on_demand: false,
        note: "the Fedora repositories",
    },
    Repo {
        id: "fedora.rpmfusion-free",
        distro: Distro::Fedora,
        label: "RPM Fusion free",
        trust: Trust::Community,
        base: false,
        default_on: true,
        on_demand: false,
        note: "codecs and extra tools",
    },
    Repo {
        id: "fedora.rpmfusion-nonfree",
        distro: Distro::Fedora,
        label: "RPM Fusion nonfree",
        trust: Trust::Community,
        base: false,
        default_on: false,
        on_demand: false,
        note: "Steam, Discord, NVIDIA drivers",
    },
    Repo {
        id: "opensuse.oss",
        distro: Distro::OpenSuse,
        label: "OSS",
        trust: Trust::Official,
        base: true,
        default_on: true,
        on_demand: false,
        note: "the openSUSE repositories",
    },
    Repo {
        id: "opensuse.non-oss",
        distro: Distro::OpenSuse,
        label: "Non-OSS",
        trust: Trust::Official,
        base: false,
        default_on: true,
        on_demand: false,
        note: "closed apps like Steam and Discord",
    },
    Repo {
        id: "opensuse.packman",
        distro: Distro::OpenSuse,
        label: "Packman",
        trust: Trust::Community,
        base: false,
        default_on: true,
        on_demand: false,
        note: "full multimedia codecs",
    },
    // NVIDIA's own repo, but configured by openSUSE itself (package openSUSE-repos-NVIDIA): the
    // distro's sanctioned way to get NVIDIA userspace. Only appears on machines with an NVIDIA GPU.
    Repo {
        id: "opensuse.nvidia",
        distro: Distro::OpenSuse,
        label: "NVIDIA",
        trust: Trust::Community,
        base: false,
        default_on: false,
        on_demand: true,
        note: "NVIDIA driver, set up the openSUSE way",
    },
];

pub fn repo(id: &str) -> Option<&'static Repo> {
    REPOS.iter().find(|r| r.id == id)
}

/// Only this distro's repos, base first.
pub fn repos_for(d: Distro) -> impl Iterator<Item = &'static Repo> {
    REPOS.iter().filter(move |r| r.distro == d)
}

/// Where one package is installed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    Repo(&'static str),
    Aur,
    Flathub,
    /// The app's own AppImage, fetched from its upstream release. Not a repo: nothing else is added.
    AppImage,
}

impl Source {
    /// Stable id used by the prototype and the installer's state file.
    pub fn key(self) -> &'static str {
        match self {
            Source::Repo(id) => id,
            Source::Aur => "aur",
            Source::Flathub => "flathub",
            Source::AppImage => "appimage",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Source::Repo(id) => repo(id).map(|r| r.label).unwrap_or(id),
            Source::Aur => "AUR",
            Source::Flathub => "Flathub",
            Source::AppImage => "AppImage",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SourceError {
    /// A repo from a different distro (or an unknown id) was asked for.
    NotThisDistro(String),
    /// The AUR only exists on Arch.
    AurNeedsArch,
    /// Base repos are always on.
    BaseIsFixed(&'static str),
    /// The detected hardware's drivers come from here.
    NeededByDrivers(Source),
}

/// The switches on page 05: this distro's optional repos, the AUR, Flathub and AppImage.
#[derive(Clone, Debug)]
pub struct Sources {
    distro: Distro,
    on: BTreeSet<&'static str>,
    aur: bool,
    flathub: bool,
    appimage: bool,
    required: BTreeSet<Source>,
}

impl Sources {
    /// Borea's defaults: native repos on, AUR on for Arch only, Flathub and AppImage on.
    pub fn defaults(distro: Distro) -> Self {
        let on = repos_for(distro).filter(|r| r.default_on).map(|r| r.id).collect();
        Sources {
            distro,
            on,
            aur: distro == Distro::Arch,
            flathub: true,
            appimage: true,
            required: BTreeSet::new(),
        }
    }

    pub fn distro(&self) -> Distro {
        self.distro
    }

    pub fn is_on(&self, s: Source) -> bool {
        match s {
            Source::Repo(id) => repo(id)
                .is_some_and(|r| r.distro == self.distro && (r.base || self.on.contains(id))),
            Source::Aur => self.distro == Distro::Arch && self.aur,
            Source::Flathub => self.flathub,
            Source::AppImage => self.appimage,
        }
    }

    /// Switch on and lock every source the drivers need ([`crate::drivers::DriverPlan::sources`]).
    pub fn require(&mut self, needed: &[Source]) -> Result<(), SourceError> {
        for &s in needed {
            self.set(s, true)?;
            self.required.insert(s);
        }
        Ok(())
    }

    pub fn is_required(&self, s: Source) -> bool {
        self.required.contains(&s)
    }

    pub fn set(&mut self, s: Source, on: bool) -> Result<(), SourceError> {
        if !on && self.required.contains(&s) {
            return Err(SourceError::NeededByDrivers(s));
        }
        match s {
            Source::Repo(id) => {
                let r = repo(id)
                    .filter(|r| r.distro == self.distro)
                    .ok_or_else(|| SourceError::NotThisDistro(id.to_string()))?;
                if r.base {
                    return if on { Ok(()) } else { Err(SourceError::BaseIsFixed(r.id)) };
                }
                if on {
                    self.on.insert(r.id);
                } else {
                    self.on.remove(r.id);
                }
            }
            Source::Aur => {
                if self.distro != Distro::Arch {
                    return Err(SourceError::AurNeedsArch);
                }
                self.aur = on;
            }
            Source::Flathub => self.flathub = on,
            Source::AppImage => self.appimage = on,
        }
        Ok(())
    }

    /// Every switch this distro offers, in display order.
    pub fn switches(&self) -> Vec<Source> {
        let mut v: Vec<Source> = repos_for(self.distro)
            .filter(|r| !r.on_demand || self.on.contains(r.id))
            .map(|r| Source::Repo(r.id))
            .collect();
        if self.distro == Distro::Arch {
            v.push(Source::Aur);
        }
        v.push(Source::Flathub);
        v.push(Source::AppImage);
        v
    }
}
