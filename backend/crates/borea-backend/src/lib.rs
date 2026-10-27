#![forbid(unsafe_code)]
//! Native distribution rules. Application/group/driver data stays in borea-catalog.
use borea_core::{Distribution as Distro, Hint, hint};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    #[serde(default)]
    pub checksum: Option<ChecksumProof>,
    pub url: String,
    pub sha256: String,
    pub signature_url: String,
    pub keyring: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bootstrap {
    pub distro: Distro,
    pub release: String,
    pub artifact: Option<Artifact>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChecksumProof {
    pub url: String,
    pub filename: String,
}
impl Bootstrap {
    pub fn validate(&self) -> Result<(), Hint> {
        if self.distro != Distro::Debian && self.artifact.is_none() {
            return Err(hint(
                "bootstrap",
                "Resolve a signed native tool root before installation.",
            ));
        }
        if let Some(a) = &self.artifact {
            if !a.url.starts_with("https://")
                || !a.signature_url.starts_with("https://")
                || a.sha256.len() != 64
                || !a.sha256.bytes().all(|c| c.is_ascii_hexdigit())
                || a.keyring != format!("/usr/share/keyrings/borea/{}.gpg", self.distro.key())
            {
                return Err(hint(
                    "bootstrap",
                    "A tool root needs HTTPS, a pinned SHA-256 and the distribution's trusted keyring.",
                ));
            }
        }
        Ok(())
    }
}
pub fn boot_packages(d: Distro) -> &'static [&'static str] {
    match d {
        Distro::Arch => &[
            "linux",
            "mkinitcpio",
            "linux-firmware",
            "systemd",
            "cryptsetup",
            "btrfs-progs",
            "dosfstools",
            "zram-generator",
            "kbd",
        ],
        Distro::Debian => &[
            "linux-image-amd64",
            "systemd-sysv",
            "cryptsetup-initramfs",
            "initramfs-tools",
            "btrfs-progs",
            "grub-efi-amd64-signed",
            "shim-signed",
            "locales",
            "systemd-zram-generator",
            "efibootmgr",
            "os-prober",
            "console-setup",
            "console-data",
            "kbd",
        ],
        Distro::Fedora => &[
            "kernel-core",
            "kernel-modules",
            "dracut",
            "cryptsetup",
            "btrfs-progs",
            "grub2-efi-x64",
            "shim-x64",
            "grub2-tools",
            "efibootmgr",
            "os-prober",
            "grubby",
            "selinux-policy-targeted",
            "policycoreutils",
            "zram-generator",
            "kbd",
        ],
        Distro::OpenSuse => &[
            "shadow",
            "kernel-default",
            "dracut",
            "cryptsetup",
            "btrfsprogs",
            "sdbootutil",
            "systemd-boot",
            "shim",
            "snapper",
            "zram-generator",
            "kbd",
        ],
    }
}
pub fn install(d: Distro, pkgs: &[String]) -> (String, Vec<String>) {
    let (tool, args): (&str, &[&str]) = match d {
        Distro::Arch => ("/usr/bin/pacman", &["-S", "--needed", "--noconfirm"]),
        Distro::Debian => ("/usr/bin/apt-get", &["install", "-y", "--no-install-recommends"]),
        Distro::Fedora => ("/usr/bin/dnf", &["install", "-y"]),
        Distro::OpenSuse => {
            ("/usr/bin/zypper", &["--non-interactive", "install", "--auto-agree-with-licenses"])
        }
    };
    (tool.into(), args.iter().map(|s| (*s).into()).chain(pkgs.iter().cloned()).collect())
}
pub fn admin_group(d: Distro) -> &'static str {
    if d == Distro::Debian { "sudo" } else { "wheel" }
}
pub fn mirror(d: Distro) -> &'static str {
    match d {
        Distro::Arch => "https://geo.mirror.pkgbuild.com/",
        Distro::Debian => "https://deb.debian.org/debian/",
        Distro::Fedora => "https://mirrors.fedoraproject.org/",
        Distro::OpenSuse => "https://download.opensuse.org/",
    }
}
pub fn apt_sources(components: &[&str]) -> String {
    format!(
        "Types: deb\nURIs: https://deb.debian.org/debian\nSuites: trixie trixie-updates\nComponents: {}\nSigned-By: /usr/share/keyrings/debian-archive-keyring.gpg\n\nTypes: deb\nURIs: https://security.debian.org/debian-security\nSuites: trixie-security\nComponents: {}\nSigned-By: /usr/share/keyrings/debian-archive-keyring.gpg\n",
        components.join(" "),
        components.join(" ")
    )
}
pub mod bootstrap;
pub mod external;
