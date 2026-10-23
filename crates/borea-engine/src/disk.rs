//! Naming a disk so it is still the same disk when the worker runs: kernel
//! names like `sda` can change between boots or when a stick is plugged in.

use std::fs;
use std::path::Path;

/// The disk's stable path: a `/dev/disk/by-id` link (serial-based) when there is
/// one, else `/dev/disk/by-path` (where it is attached), relative to `root`.
/// Virtual disks without a serial have only the second.
pub fn stable_path(root: &Path, name: &str) -> Option<String> {
    ["by-id", "by-path"].into_iter().find_map(|kind| {
        let dir = root.join("dev/disk").join(kind);
        let mut links: Vec<String> = fs::read_dir(&dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| {
                fs::read_link(e.path())
                    .is_ok_and(|t| t.file_name().is_some_and(|n| n.to_string_lossy() == name))
            })
            .map(|e| e.file_name().to_string_lossy().into_owned())
            // wwn- names describe the medium less clearly than the vendor's own.
            .filter(|n| !n.starts_with("wwn-") || kind == "by-path")
            .collect();
        links.sort();
        links.first().map(|link| format!("/dev/disk/{kind}/{link}"))
    })
}

/// The kernel name a stable path points to now, relative to `root`.
pub fn resolve(root: &Path, stable_path: &str) -> Option<String> {
    let link = root.join(stable_path.trim_start_matches('/'));
    let target = fs::read_link(link).ok()?;
    Some(target.file_name()?.to_string_lossy().into_owned())
}

/// Partitions of `disk` by number, from sysfs: `vda1`, or `nvme0n1p1`.
pub fn partition(root: &Path, disk: &str, number: u32) -> Option<String> {
    let dir = root.join("sys/block").join(disk);
    fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).find_map(|e| {
        let n: u32 = fs::read_to_string(e.path().join("partition")).ok()?.trim().parse().ok()?;
        (n == number).then(|| e.file_name().to_string_lossy().into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use borea_testkit::FakeRoot;

    #[test]
    fn prefers_serials_over_wwn_and_falls_back_to_the_path() {
        let root = FakeRoot::new();
        root.link("dev/disk/by-id/wwn-0x5000", "dev/sda")
            .link("dev/disk/by-id/ata-Samsung_SSD_870_S5Y", "dev/sda")
            .link("dev/disk/by-id/ata-Samsung_SSD_870_S5Y-part1", "dev/sda1")
            .link("dev/disk/by-path/pci-0000:00:04.0", "dev/vda");
        assert_eq!(
            stable_path(root.path(), "sda").as_deref(),
            Some("/dev/disk/by-id/ata-Samsung_SSD_870_S5Y")
        );
        assert_eq!(
            stable_path(root.path(), "vda").as_deref(),
            Some("/dev/disk/by-path/pci-0000:00:04.0")
        );
        assert_eq!(stable_path(root.path(), "sdb"), None);
        assert_eq!(
            resolve(root.path(), "/dev/disk/by-path/pci-0000:00:04.0").as_deref(),
            Some("vda")
        );
    }

    #[test]
    fn finds_partitions_by_number() {
        let root = FakeRoot::new();
        root.file("sys/block/nvme0n1/nvme0n1p1/partition", "1\n")
            .file("sys/block/nvme0n1/nvme0n1p2/partition", "2\n");
        assert_eq!(partition(root.path(), "nvme0n1", 2).as_deref(), Some("nvme0n1p2"));
        assert_eq!(partition(root.path(), "nvme0n1", 3), None);
    }
}
