//! Shared Borea domain types.
//!
//! Borea separates three things that a distribution ISO normally fuses together:
//!
//! - the **installation environment** (the host Borea runs in),
//! - **Windpad** (the same interface for every target), and
//! - **distribution adapters** (how each target system is obtained and configured).
//!
//! Types here are shared by all three and carry no UI or privileged behavior.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A discovered value whose absence is meaningful.
///
/// `Unknown` means "we could not tell"; it is never the same as "unsupported".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "kebab-case")]
pub enum Fact<T> {
    Known(T),
    Unknown { reason: String },
}

impl<T> Fact<T> {
    pub fn unknown(reason: impl Into<String>) -> Self {
        Fact::Unknown { reason: reason.into() }
    }

    pub fn known(&self) -> Option<&T> {
        match self {
            Fact::Known(value) => Some(value),
            Fact::Unknown { .. } => None,
        }
    }
}

impl<T: fmt::Display> fmt::Display for Fact<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fact::Known(value) => value.fmt(f),
            Fact::Unknown { reason } => write!(f, "unknown ({reason})"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Architecture {
    X86_64,
    Aarch64,
    Other,
}

impl Architecture {
    pub fn from_uname(machine: &str) -> Self {
        match machine {
            "x86_64" | "amd64" => Architecture::X86_64,
            "aarch64" | "arm64" => Architecture::Aarch64,
            _ => Architecture::Other,
        }
    }
}

impl fmt::Display for Architecture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Architecture::X86_64 => "x86-64",
            Architecture::Aarch64 => "aarch64",
            Architecture::Other => "other",
        })
    }
}

/// x86-64 micro-architecture levels used by optimized repositories and CPU baselines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum X86Level {
    #[serde(rename = "x86-64")]
    V1,
    #[serde(rename = "x86-64-v2")]
    V2,
    #[serde(rename = "x86-64-v3")]
    V3,
    #[serde(rename = "x86-64-v4")]
    V4,
}

impl fmt::Display for X86Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            X86Level::V1 => "x86-64",
            X86Level::V2 => "x86-64-v2",
            X86Level::V3 => "x86-64-v3",
            X86Level::V4 => "x86-64-v4",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FirmwareMode {
    Uefi { bits: Option<u8> },
    Bios,
}

impl fmt::Display for FirmwareMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FirmwareMode::Uefi { bits: Some(bits) } => write!(f, "UEFI ({bits}-bit)"),
            FirmwareMode::Uefi { bits: None } => f.write_str("UEFI"),
            FirmwareMode::Bios => f.write_str("BIOS"),
        }
    }
}

/// The four Windpad family cards. Grouping is navigation only: a derivative keeps
/// its own release channel and deployment model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    Arch,
    Fedora,
    Debian,
    Opensuse,
}

impl Family {
    pub const ALL: [Family; 4] = [Family::Arch, Family::Fedora, Family::Debian, Family::Opensuse];
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Family::Arch => "Arch",
            Family::Fedora => "Fedora",
            Family::Debian => "Debian",
            Family::Opensuse => "openSUSE",
        })
    }
}

/// Stable identifier of a target distribution, e.g. `cachyos`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DistroId(pub String);

impl DistroId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DistroId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Which upstream channel "latest" means for a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseChannel {
    /// Current rolling repositories (Arch, CachyOS, PikaOS).
    Rolling,
    /// A coherent published snapshot of a rolling distribution (Tumbleweed).
    Snapshot,
    /// Latest general-availability stable release (Fedora, Debian, Nobara, Omarchy).
    Stable,
    /// A published image stream pinned by digest (Bazzite).
    ImageStream,
}

impl fmt::Display for ReleaseChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ReleaseChannel::Rolling => "latest rolling",
            ReleaseChannel::Snapshot => "latest published snapshot",
            ReleaseChannel::Stable => "latest stable",
            ReleaseChannel::ImageStream => "latest stable image",
        })
    }
}

/// Certification state of a catalog entry. Menu visibility never implies readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SupportStatus {
    Planned,
    IntegrationPending,
    Experimental,
    Supported,
}

impl SupportStatus {
    /// Only supported entries may execute in the stable channel.
    pub fn installable_in_stable(self) -> bool {
        self == SupportStatus::Supported
    }
}

impl fmt::Display for SupportStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SupportStatus::Planned => "planned",
            SupportStatus::IntegrationPending => "integration pending",
            SupportStatus::Experimental => "experimental",
            SupportStatus::Supported => "supported",
        })
    }
}

/// Coarse device class used for edition recommendations (e.g. handheld editions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceClass {
    Desktop,
    Laptop,
    Handheld,
    Tablet,
    Server,
    VirtualMachine,
}

impl fmt::Display for DeviceClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DeviceClass::Desktop => "desktop",
            DeviceClass::Laptop => "laptop",
            DeviceClass::Handheld => "handheld",
            DeviceClass::Tablet => "tablet",
            DeviceClass::Server => "server",
            DeviceClass::VirtualMachine => "virtual machine",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_not_known() {
        let fact: Fact<u8> = Fact::unknown("no sysfs");
        assert_eq!(fact.known(), None);
        assert_eq!(fact.to_string(), "unknown (no sysfs)");
    }

    #[test]
    fn x86_levels_order() {
        assert!(X86Level::V3 > X86Level::V2);
        assert_eq!(X86Level::V3.to_string(), "x86-64-v3");
    }

    #[test]
    fn only_supported_installs() {
        assert!(!SupportStatus::Experimental.installable_in_stable());
        assert!(SupportStatus::Supported.installable_in_stable());
    }
}
