//! Extras: optional features each card offers as checkboxes, such as Gaming or
//! Media codecs. One idea per extra; every distribution provides it its own way.

use std::collections::BTreeMap;

use borea_live_core::DistroId;
use serde::Deserialize;

/// Extras bundled with this build. Unsigned research data.
pub const BUNDLED: &str = include_str!("../data/extras.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extras {
    pub schema: u32,
    pub reviewed: String,
    #[serde(rename = "extra")]
    pub extras: Vec<Extra>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extra {
    pub id: String,
    pub name: String,
    pub summary: String,
    /// How each distribution provides it. Absent means not offered there.
    pub on: BTreeMap<DistroId, Provision>,
}

/// One distribution's way of providing an extra.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provision {
    /// Packages ticking the extra installs. Empty when it is built in.
    #[serde(default)]
    pub packages: Vec<String>,
    /// Replaces the extra's summary when these packages bring something different.
    #[serde(default)]
    pub summary: Option<String>,
    /// A repository the packages need beyond the defaults, named in review.
    #[serde(default)]
    pub needs: Option<String>,
    /// Ticked to begin with.
    #[serde(default)]
    pub default: bool,
    /// Every edition already has it: nothing to tick.
    #[serde(default)]
    pub built_in: bool,
}

/// An extra as one distribution offers it.
#[derive(Debug, Clone, Copy)]
pub struct Offered<'a> {
    pub extra: &'a Extra,
    pub provision: &'a Provision,
}

impl Offered<'_> {
    pub fn summary(&self) -> &str {
        self.provision.summary.as_deref().unwrap_or(&self.extra.summary)
    }

    /// The summary, plus the repository it needs: what the checkbox says.
    pub fn describe(&self) -> String {
        match &self.provision.needs {
            Some(needs) => format!("{} · enables {needs}", self.summary()),
            None => self.summary().to_owned(),
        }
    }
}

impl Extras {
    /// Extras offered on `distro`, in file order.
    pub fn on<'a>(&'a self, distro: &DistroId) -> impl Iterator<Item = Offered<'a>> + use<'a> {
        let distro = distro.clone();
        self.extras
            .iter()
            .filter_map(move |extra| Some(Offered { extra, provision: extra.on.get(&distro)? }))
    }

    pub fn get(&self, id: &str) -> Option<&Extra> {
        self.extras.iter().find(|e| e.id == id)
    }

    /// What is wrong with an entry, if anything.
    pub(crate) fn problem(provision: &Provision) -> Option<&'static str> {
        match (provision.packages.is_empty(), provision.built_in) {
            (false, true) => Some("packages to add, yet built in"),
            (true, false) => Some("nothing to install and not built in"),
            (true, true) if provision.default => Some("built in, yet ticked by default"),
            _ => None,
        }
    }
}
