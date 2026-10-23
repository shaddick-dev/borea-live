//! Which disks may receive the image, and why the others may not.
//!
//! Only removable or USB disks are ever listed. Each gets a verdict; only `Ready`
//! can be written, and only after the user types its name.

use std::collections::BTreeSet;
use std::fmt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use borea_hardware::{Disk, DiskTransport, DiskUsage, Scanner};
use borea_host::HostKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Ready,
    /// Has mounted filesystems; unmounting them makes it ready.
    Mounted(Vec<String>),
    /// Never writable, with the reason shown to the user.
    Refused(String),
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Ready => f.write_str("ready"),
            Verdict::Mounted(at) if at.is_empty() => {
                f.write_str("an installer image on it is mounted")
            }
            Verdict::Mounted(at) => write!(f, "mounted at {}", at.join(", ")),
            Verdict::Refused(why) => f.write_str(why),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub disk: Disk,
    pub verdict: Verdict,
}

impl Target {
    pub fn describe(&self) -> String {
        let model = self.disk.model.as_deref().unwrap_or("USB disk");
        format!("{model} · {} · {}", self.disk.name, human(self.disk.size_bytes))
    }
}

/// Only removable media and USB disks are candidates for flashing.
pub fn is_candidate(disk: &Disk) -> bool {
    disk.removable || disk.transport == DiskTransport::Usb
}

/// Judge one disk for an image of `image_size` bytes. Pure.
pub fn assess(
    disk: &Disk,
    image_size: u64,
    image_disks: &BTreeSet<String>,
    host: &HostKind,
) -> Verdict {
    if image_disks.contains(&disk.name) {
        return Verdict::Refused("holds the image you are writing".into());
    }
    if disk.size_bytes < image_size {
        return Verdict::Refused(format!("too small: the image needs {}", human(image_size)));
    }
    match &disk.usage {
        DiskUsage::Free => Verdict::Ready,
        DiskUsage::HostRoot => Verdict::Refused("runs this system".into()),
        // In a live session, an installer medium is most likely the one we booted from.
        DiskUsage::InstallationMedium if host.is_live() => {
            Verdict::Refused("this live session may be running from it".into())
        }
        DiskUsage::InstallationMedium => Verdict::Mounted(Vec::new()),
        DiskUsage::InUse(at) if at.iter().any(|t| t == "[swap]") => {
            Verdict::Refused("used as swap".into())
        }
        DiskUsage::InUse(at) => Verdict::Mounted(at.clone()),
    }
}

/// Every flashable candidate on this machine, judged for `image`.
pub fn discover(root: &Path, image: &Path, image_size: u64, host: &HostKind) -> Vec<Target> {
    let image_disks = disks_holding(root, image);
    Scanner::new(root)
        .disks()
        .into_iter()
        .filter(is_candidate)
        .map(|disk| Target { verdict: assess(&disk, image_size, &image_disks, host), disk })
        .collect()
}

/// The physical disks a file is stored on, following partitions and stacked
/// devices (LUKS, LVM) down through sysfs `slaves`.
pub fn disks_holding(root: &Path, file: &Path) -> BTreeSet<String> {
    let Ok(meta) = std::fs::metadata(file) else { return BTreeSet::new() };
    let dev = meta.dev();
    let (major, minor) =
        ((dev >> 8) & 0xfff | (dev >> 32) & !0xfff, dev & 0xff | (dev >> 12) & !0xff);
    let link = root.join(format!("sys/dev/block/{major}:{minor}"));
    let Ok(target) = std::fs::read_link(&link) else { return BTreeSet::new() };
    let node = root.join("sys/dev/block").join(target);
    let mut disks = BTreeSet::new();
    underlying(root, &node, &mut disks);
    disks
}

fn underlying(root: &Path, node: &Path, disks: &mut BTreeSet<String>) {
    let name =
        |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let slaves: Vec<PathBuf> = std::fs::read_dir(node.join("slaves"))
        .map(|dir| dir.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    if !slaves.is_empty() {
        for slave in slaves {
            underlying(root, &root.join("sys/class/block").join(name(&slave)), disks);
        }
    } else if node.join("partition").exists() {
        // A partition's parent directory is its disk.
        let resolved = std::fs::canonicalize(node).unwrap_or_else(|_| node.into());
        if let Some(parent) = resolved.parent() {
            disks.insert(name(parent));
        }
    } else {
        disks.insert(name(node));
    }
}

/// Bytes as the size printed on the stick: decimal gigabytes.
pub fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1_000_000_000 => format!("{:.1} GB", b as f64 / 1e9),
        b if b >= 1_000_000 => format!("{:.0} MB", b as f64 / 1e6),
        b => format!("{b} B"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use borea_testkit::FakeRoot;

    fn stick(name: &str, size: u64, usage: DiskUsage) -> Disk {
        Disk {
            name: name.into(),
            model: Some("Example Stick".into()),
            size_bytes: size,
            removable: true,
            rotational: None,
            transport: DiskTransport::Usb,
            partitions: vec![],
            usage,
        }
    }

    const GB: u64 = 1_000_000_000;

    #[test]
    fn verdicts() {
        let none = BTreeSet::new();
        let installed = HostKind::InstalledSystem;
        let live = HostKind::LiveSession;
        assert_eq!(
            assess(&stick("sdb", 32 * GB, DiskUsage::Free), 2 * GB, &none, &installed),
            Verdict::Ready
        );
        assert!(
            matches!(assess(&stick("sdb", GB, DiskUsage::Free), 2 * GB, &none, &installed), Verdict::Refused(r) if r.contains("too small"))
        );
        assert!(matches!(
            assess(&stick("sdb", 32 * GB, DiskUsage::HostRoot), GB, &none, &installed),
            Verdict::Refused(_)
        ));
        assert_eq!(
            assess(
                &stick("sdb", 32 * GB, DiskUsage::InUse(vec!["/run/media/me/STICK".into()])),
                GB,
                &none,
                &installed
            ),
            Verdict::Mounted(vec!["/run/media/me/STICK".into()])
        );
        assert!(matches!(
            assess(
                &stick("sdb", 32 * GB, DiskUsage::InUse(vec!["[swap]".into()])),
                GB,
                &none,
                &installed
            ),
            Verdict::Refused(_)
        ));
        // An old installer stick can be reused from an installed system, never from a live one.
        assert!(matches!(
            assess(&stick("sdb", 32 * GB, DiskUsage::InstallationMedium), GB, &none, &installed),
            Verdict::Mounted(_)
        ));
        assert!(matches!(
            assess(&stick("sdb", 32 * GB, DiskUsage::InstallationMedium), GB, &none, &live),
            Verdict::Refused(_)
        ));
        let holding = BTreeSet::from(["sdb".to_owned()]);
        assert!(
            matches!(assess(&stick("sdb", 32 * GB, DiskUsage::Free), GB, &holding, &installed), Verdict::Refused(r) if r.contains("holds the image"))
        );
    }

    #[test]
    fn internal_disks_are_never_candidates() {
        let mut internal = stick("nvme0n1", 1000 * GB, DiskUsage::Free);
        internal.removable = false;
        internal.transport = DiskTransport::Nvme;
        assert!(!is_candidate(&internal));
        assert!(is_candidate(&stick("sdb", 32 * GB, DiskUsage::Free)));
    }

    #[test]
    fn follows_partitions_and_encryption_to_the_disk() {
        // A file on dm-0 (LUKS) on nvme0n1p2, recorded as sysfs symlinks.
        let root = FakeRoot::new();
        root.file("sys/devices/virtual/block/dm-0/dev", "254:0\n")
            .dir("sys/devices/pci/nvme/nvme0n1/nvme0n1p2")
            .file("sys/devices/pci/nvme/nvme0n1/nvme0n1p2/partition", "2\n")
            .link(
                "sys/devices/virtual/block/dm-0/slaves/nvme0n1p2",
                "sys/devices/pci/nvme/nvme0n1/nvme0n1p2",
            )
            .link("sys/class/block/nvme0n1p2", "sys/devices/pci/nvme/nvme0n1/nvme0n1p2");
        let mut disks = BTreeSet::new();
        underlying(root.path(), &root.path().join("sys/devices/virtual/block/dm-0"), &mut disks);
        assert_eq!(disks, BTreeSet::from(["nvme0n1".to_owned()]));
    }

    #[test]
    fn sizes_read_like_the_label_on_the_stick() {
        assert_eq!(human(32 * GB), "32.0 GB");
        assert_eq!(human(700_000_000), "700 MB");
    }
}
