//! The installation plan: everything an install will do, resolved and pinned
//! before anything is written. The worker carries out exactly this, after
//! checking the digest the user approved.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Bumped whenever a field changes meaning; the worker refuses other schemas.
pub const SCHEMA: u32 = 1;

/// What the user asked for. Holds no secrets: the password goes straight to
/// the worker, never into a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRequest {
    pub distro: String,
    pub edition: String,
    /// Kernel name of the disk to erase, e.g. `vda` or `nvme0n1`.
    pub disk: String,
    pub hostname: String,
    pub username: String,
    pub timezone: String,
    pub locale: String,
    pub keymap: String,
    /// Extras ticked on the card, by id.
    #[serde(default)]
    pub extras: Vec<String>,
    /// Added to the kernel command line, e.g. `console=ttyS0,115200`.
    #[serde(default)]
    pub kernel_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub schema: u32,
    pub distro: String,
    pub edition: String,
    pub disk: TargetDisk,
    pub layout: Layout,
    /// The target's own tools, from its upstream: verified before the plan exists.
    pub bootstrap: Artifact,
    /// Package mirror, used for the whole install.
    pub mirror: String,
    pub packages: Vec<String>,
    pub system: System,
    pub boot: Boot,
    /// systemd units enabled on the new system.
    pub services: Vec<String>,
}

/// The disk to erase, as it was when planned. The worker finds it again by
/// `stable_path` and refuses if anything here no longer matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetDisk {
    pub name: String,
    /// `/dev/disk/by-id/…`, else `/dev/disk/by-path/…`: never the bare kernel name.
    pub stable_path: String,
    pub size_bytes: u64,
    pub model: Option<String>,
}

/// GPT: an EFI system partition mounted at /boot, then ext4 root on the rest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    pub esp_mib: u64,
    pub root_fs: String,
}

/// A verified download.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Upstream's release name, e.g. `2026.10.01`.
    pub release: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    /// Fingerprint of the key whose signature was checked.
    pub signed_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct System {
    pub hostname: String,
    pub timezone: String,
    pub locale: String,
    pub keymap: String,
    /// Created with administrator rights (the `wheel` group, through sudo).
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Boot {
    pub loader: String,
    /// The microcode image Limine loads before the initramfs, if any.
    pub microcode: Option<String>,
    pub kernel_args: Vec<String>,
}

impl InstallPlan {
    /// SHA-256 of the plan as serialized: what the user approves, and what the
    /// worker checks before it touches the disk.
    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("a plan always serializes");
        hex(&Sha256::digest(bytes))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a plan always serializes")
    }

    pub fn from_json(text: &str) -> Result<InstallPlan, String> {
        let plan: InstallPlan = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if plan.schema != SCHEMA {
            return Err(format!("plan schema {} is not {SCHEMA}", plan.schema));
        }
        Ok(plan)
    }

    /// The review, in order: what will happen, in plain words.
    pub fn review(&self) -> Vec<String> {
        let d = &self.disk;
        let model = d.model.as_deref().map(|m| format!(" ({m})")).unwrap_or_default();
        vec![
            format!(
                "Erase {}{model}, {} GB: every partition and file on it is lost",
                d.name,
                d.size_bytes / 1_000_000_000
            ),
            format!(
                "GPT: EFI system partition {} MiB at /boot, {} root on the rest",
                self.layout.esp_mib, self.layout.root_fs
            ),
            format!(
                "Arch Linux tools from the signed bootstrap {} (key {})",
                self.bootstrap.release,
                short(&self.bootstrap.signed_by)
            ),
            format!("Install {} from {}", self.packages.join(" "), self.mirror),
            format!(
                "{} · {} · {} · keyboard {}",
                self.system.hostname, self.system.timezone, self.system.locale, self.system.keymap
            ),
            format!("User {} with administrator rights; root login locked", self.system.username),
            format!("Enable {}", self.services.join(", ")),
            format!(
                "Boot with Limine from the EFI partition{}",
                self.boot
                    .microcode
                    .as_deref()
                    .map(|m| format!(", loading {m}"))
                    .unwrap_or_default()
            ),
        ]
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The last 16 hex digits of a fingerprint, as OpenPGP tools show key IDs.
fn short(fingerprint: &str) -> &str {
    &fingerprint[fingerprint.len().saturating_sub(16)..]
}
