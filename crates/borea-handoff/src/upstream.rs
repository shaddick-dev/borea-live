//! The installers Borea hands off to, pinned to a release Borea was tested with.

/// One distribution's own installer ISO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Upstream {
    /// Borea's key for it, as the GUI and `borea-cli` name it.
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    /// Where `<iso>`, `<iso>.sha256` and `<iso>.sig` are published.
    pub base: &'static str,
    pub iso: &'static str,
    /// The pinned public key that must have signed the ISO, in the live image.
    pub keyring: &'static str,
    pub fingerprint: &'static str,
    /// The loopback boot configuration inside the ISO, and the menu entry in it
    /// that boots the installer.
    pub boot_config: &'static str,
    pub boot_entry: &'static str,
    /// The installer can install into free space left beside another system.
    pub alongside: bool,
    /// The least space its installer accepts, whole disk or alongside.
    pub minimum: u64,
    /// Extras Borea adds quietly on the installed system's first start.
    pub extras: &'static [Extra],
}

/// An extra a hand-off target can be given: the same packages as Borea Live's
/// catalog offers for it (crates/borea-live-catalog/data/extras.toml; a test
/// keeps the two the same).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extra {
    pub id: &'static str,
    pub name: &'static str,
    pub packages: &'static [&'static str],
    /// Its packages come from Arch's multilib repository.
    pub multilib: bool,
}

const OMARCHY_EXTRAS: &[Extra] = &[
    Extra {
        id: "gaming",
        name: "Gaming",
        packages: &[
            "steam",
            "lutris",
            "wine",
            "gamemode",
            "lib32-gamemode",
            "mangohud",
            "lib32-mangohud",
            "gamescope",
        ],
        multilib: true,
    },
    Extra {
        id: "codecs",
        name: "Media codecs",
        packages: &[
            "ffmpeg",
            "gst-libav",
            "gst-plugins-good",
            "gst-plugins-bad",
            "gst-plugins-ugly",
        ],
        multilib: false,
    },
    Extra { id: "flatpak", name: "Flatpak", packages: &["flatpak"], multilib: false },
    Extra {
        id: "printing",
        name: "Printing",
        packages: &["cups", "system-config-printer"],
        multilib: false,
    },
];

impl Upstream {
    pub fn extra(&self, id: &str) -> Option<&'static Extra> {
        self.extras.iter().find(|e| e.id == id)
    }
}

impl Upstream {
    pub fn iso_url(&self) -> String {
        format!("{}/{}", self.base, self.iso)
    }
    pub fn sha256_url(&self) -> String {
        format!("{}/{}.sha256", self.base, self.iso)
    }
    pub fn sig_url(&self) -> String {
        format!("{}/{}.sig", self.base, self.iso)
    }
}

/// Omarchy's release ISO. Its installer offers a full-disk install and a
/// free-space install (beside Windows or another OS), both LUKS-encrypted.
/// Signing key: `builder/omarchy.gpg` in omacom-io/omarchy-iso, matching the
/// fingerprint its README publishes (see backend/dist/keys/PROVENANCE.md).
pub const OMARCHY: Upstream = Upstream {
    id: "omarchy",
    name: "Omarchy",
    version: "4.0.6",
    base: "https://iso.omarchy.org",
    iso: "omarchy-4.0.6.iso",
    keyring: "/usr/share/keyrings/borea/omarchy.gpg",
    fingerprint: "40DFB630FF42BCFFB047046CF0134EE680CAC571",
    boot_config: "boot/grub/loopback.cfg",
    boot_entry: "archlinux",
    alongside: true,
    // The configurator's MIN_INSTALL_B: its 2 GiB ESP plus the system.
    minimum: 32 << 30,
    extras: OMARCHY_EXTRAS,
};

pub const ALL: &[Upstream] = &[OMARCHY];

pub fn find(id: &str) -> Option<&'static Upstream> {
    ALL.iter().find(|u| u.id == id)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn omarchy_is_pinned_and_signed() {
        let o = find("omarchy").unwrap_or(&OMARCHY);
        assert_eq!(o.iso_url(), "https://iso.omarchy.org/omarchy-4.0.6.iso");
        assert_eq!(o.sig_url(), "https://iso.omarchy.org/omarchy-4.0.6.iso.sig");
        assert_eq!(o.fingerprint.len(), 40);
        assert!(o.iso.contains(o.version));
        assert!(find("windows").is_none());
    }

    /// Borea Live offers exactly these, from its catalog: the hand-off must add
    /// the same packages the checkbox promised.
    #[test]
    fn omarchy_extras_match_the_catalog() {
        let catalog: toml::Value =
            toml::from_str(include_str!("../../borea-live-catalog/data/extras.toml")).unwrap();
        let offered: Vec<&toml::Value> = catalog["extra"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["on"].get("omarchy").is_some())
            .collect();
        assert_eq!(offered.len(), OMARCHY.extras.len(), "every offered extra is pinned");
        for entry in offered {
            let id = entry["id"].as_str().unwrap();
            let pinned = OMARCHY.extra(id).unwrap_or_else(|| panic!("{id} is not pinned"));
            let on = &entry["on"]["omarchy"];
            let packages: Vec<&str> =
                on["packages"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
            assert_eq!(packages, pinned.packages, "{id}");
            let multilib =
                on.get("needs").and_then(|n| n.as_str()).is_some_and(|n| n.contains("multilib"));
            assert_eq!(multilib, pinned.multilib, "{id}");
        }
    }
}
