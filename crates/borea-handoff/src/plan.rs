//! What a hand-off will do, reviewed and approved before anything changes.

use borea_core::{DiskPlan, GIB, Hint, hint};
use borea_disk::{Change, DiskFacts};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::upstream::{self, Upstream};

/// The user's choice: which installer, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handoff {
    pub version: u32,
    pub target: String,
    pub disk: DiskPlan,
    /// Extras (by id) to add on the installed system's first start.
    #[serde(default)]
    pub extras: Vec<String>,
}

/// Everything the user approves, in the order it happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub target: String,
    pub name: String,
    pub version: String,
    pub iso_url: String,
    pub whole_disk: bool,
    pub disk: String,
    /// The reviewed disk's device and logical sector size, for its changes.
    pub disk_path: std::path::PathBuf,
    pub sector: u64,
    /// What Borea itself changes on the disk before handing off.
    pub changes: Vec<Change>,
    pub steps: Vec<String>,
    /// What to pick in the installer so it does what was chosen here.
    pub choose: String,
    /// Extras added on the installed system's first start, by id.
    #[serde(default)]
    pub extras: Vec<String>,
}

impl Review {
    /// The approval `handoff.start` must quote: any change to the plan, the disk
    /// or the pinned release makes it a different review.
    pub fn digest(&self) -> Result<String, Hint> {
        let bytes = serde_json::to_vec(self).map_err(|e| hint("handoff", e.to_string()))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

fn gib(bytes: u64) -> String {
    format!("{:.0} GiB", bytes as f64 / GIB as f64)
}

pub fn review(h: &Handoff, facts: &DiskFacts) -> Result<Review, Hint> {
    if h.version != 1 {
        return Err(hint("version", "This hand-off plan version is not supported."));
    }
    let up: &Upstream = upstream::find(&h.target)
        .ok_or_else(|| hint("target", format!("Borea can't hand off to {}.", h.target)))?;
    let whole_disk = matches!(h.disk, DiskPlan::Whole { .. });
    if !whole_disk && !up.alongside {
        return Err(hint("disk", format!("{}'s installer can only use a whole disk.", up.name)));
    }
    let changes = borea_disk::make_space(&h.disk, facts, up.minimum)?;
    let id = h.disk.disk();
    // Some disks report no model; the device name still identifies them.
    let model = if id.model.trim().is_empty() {
        id.path.display().to_string()
    } else {
        id.model.trim().to_owned()
    };
    let disk = format!("{model} · {} · {}", id.path.display(), gib(id.size));
    let mut steps = vec![
        format!("Download {} {} to the Borea stick: {}", up.name, up.version, up.iso_url()),
        "If the stick has no data partition yet, add one in its free space after Borea Live \
         (Borea Live itself is not touched)"
            .to_owned(),
        format!(
            "Check it against its published SHA-256 and {}'s signing key {}",
            up.name, up.fingerprint
        ),
    ];
    let choose = match &h.disk {
        DiskPlan::Whole { .. } => {
            steps.push(format!("Leave {disk} unchanged until {}'s installer erases it", up.name));
            format!(
                "In {}'s installer, choose {} and a full-disk install. It erases the disk after its own confirmation.",
                up.name, model
            )
        }
        DiskPlan::Alongside { linux_size, .. } => {
            steps.push(format!(
                "Shrink Windows on {disk} by {}, leaving that much free space after it",
                gib(*linux_size)
            ));
            format!(
                "In {}'s installer, choose {} and \"Free space install\". It installs only into the {} Borea freed, beside Windows.",
                up.name,
                model,
                gib(*linux_size)
            )
        }
    };
    let mut extras = Vec::new();
    for id in &h.extras {
        let extra = up
            .extra(id)
            .ok_or_else(|| hint("extras", format!("Borea can't add \"{id}\" to {}.", up.name)))?;
        if !extras.contains(&extra.id.to_owned()) {
            extras.push(extra.id.to_owned());
        }
    }
    if !extras.is_empty() {
        let names: Vec<&str> =
            extras.iter().filter_map(|id| up.extra(id)).map(|e| e.name).collect();
        let multilib = extras.iter().filter_map(|id| up.extra(id)).any(|e| e.multilib);
        steps.push(format!(
            "When {} first starts, add {} quietly from Arch's repositories{}",
            up.name,
            names.join(", "),
            if multilib { ", turning on its multilib repository" } else { "" }
        ));
    }
    steps.push(format!("Restart straight into {}'s installer", up.name));
    Ok(Review {
        target: up.id.into(),
        name: up.name.into(),
        version: up.version.into(),
        iso_url: up.iso_url(),
        whole_disk,
        disk,
        disk_path: id.path.clone(),
        sector: id.logical_sector,
        changes,
        steps,
        choose,
        extras,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use borea_core::{DiskId, PartId};
    use borea_disk::{Disk, Partition};

    fn id() -> DiskId {
        DiskId {
            logical_sector: 512,
            path: "/dev/nvme0n1".into(),
            serial: Some("S6P7NF0W123456".into()),
            size: 512 * GIB,
            model: "Samsung 980".into(),
        }
    }

    fn windows() -> Partition {
        Partition {
            path: "/dev/nvme0n1p3".into(),
            number: 3,
            uuid: "WINDOWS".into(),
            start: GIB,
            size: 400 * GIB,
            fs: "ntfs".into(),
            efi: false,
            free: None,
            bitlocker: false,
            ntfs_checked: true,
            ntfs_hibernated: false,
            ntfs_dirty: false,
            ntfs_min_size: Some(100 * GIB),
        }
    }

    fn facts() -> DiskFacts {
        DiskFacts {
            uefi: true,
            disks: vec![Disk {
                id: id(),
                readonly: false,
                in_use: false,
                live: false,
                removable: false,
                transport: "nvme".into(),
                parts: vec![windows()],
            }],
        }
    }

    fn alongside(size: u64) -> Handoff {
        Handoff {
            version: 1,
            target: "omarchy".into(),
            disk: DiskPlan::Alongside {
                disk: id(),
                windows_part: PartId { path: windows().path, uuid: "WINDOWS".into(), number: 3 },
                linux_size: size,
            },
            extras: Vec::new(),
        }
    }

    #[test]
    fn whole_disk_changes_nothing_before_omarchys_installer() {
        let h = Handoff {
            version: 1,
            target: "omarchy".into(),
            disk: DiskPlan::Whole { disk: id() },
            extras: Vec::new(),
        };
        let r = review(&h, &facts()).unwrap();
        assert!(r.whole_disk && r.changes.is_empty());
        assert!(r.choose.contains("full-disk install"));
        assert!(r.steps.iter().any(|s| s.contains("unchanged")));
    }

    #[test]
    fn alongside_shrinks_windows_by_exactly_the_chosen_size() {
        let r = review(&alongside(60 * GIB), &facts()).unwrap();
        assert!(!r.whole_disk);
        assert_eq!(r.changes.len(), 1);
        let Change::ResizeNtfs { new_size, .. } = &r.changes[0] else { unreachable!() };
        assert_eq!(*new_size, 340 * GIB);
        assert!(r.choose.contains("Free space install") && r.choose.contains("60 GiB"));
        assert!(review(&alongside(20 * GIB), &facts()).is_err(), "below Omarchy's 32 GiB");
    }

    #[test]
    fn a_disk_without_a_model_is_named_by_its_device() {
        let mut nameless = id();
        nameless.model = String::new();
        let mut f = facts();
        f.disks[0].id = nameless.clone();
        let h = Handoff {
            version: 1,
            target: "omarchy".into(),
            disk: DiskPlan::Whole { disk: nameless },
            extras: Vec::new(),
        };
        let r = review(&h, &f).unwrap();
        assert!(r.disk.starts_with("/dev/nvme0n1 · "), "{}", r.disk);
        assert!(r.choose.contains("choose /dev/nvme0n1 and"), "{}", r.choose);
    }

    #[test]
    fn approval_changes_with_anything_that_matters() {
        let a = review(&alongside(60 * GIB), &facts()).unwrap().digest().unwrap();
        let b = review(&alongside(61 * GIB), &facts()).unwrap().digest().unwrap();
        assert_ne!(a, b);
        let mut other = alongside(60 * GIB);
        other.target = "nothing".into();
        assert!(review(&other, &facts()).is_err());
        other.target = "omarchy".into();
        other.version = 2;
        assert!(review(&other, &facts()).is_err());
        let mut extras = alongside(60 * GIB);
        extras.extras = vec!["printing".into()];
        assert_ne!(a, review(&extras, &facts()).unwrap().digest().unwrap());
    }

    #[test]
    fn extras_are_named_in_review_and_unknown_ones_refused() {
        let mut h = alongside(60 * GIB);
        h.extras = vec!["gaming".into(), "printing".into(), "gaming".into()];
        let r = review(&h, &facts()).unwrap();
        assert_eq!(r.extras, ["gaming", "printing"], "once each, in order");
        let step = r.steps.iter().find(|s| s.contains("first starts")).unwrap();
        assert!(step.contains("Gaming, Printing") && step.contains("multilib"), "{step}");
        h.extras = vec!["printing".into()];
        let r = review(&h, &facts()).unwrap();
        assert!(!r.steps.iter().any(|s| s.contains("multilib")));
        h.extras = vec!["rootkit".into()];
        assert!(review(&h, &facts()).is_err());
        // Plans from before extras existed still read.
        let old: Handoff = serde_json::from_value(serde_json::json!({
            "version": 1, "target": "omarchy",
            "disk": serde_json::to_value(alongside(60 * GIB).disk).unwrap()
        }))
        .unwrap();
        assert!(old.extras.is_empty());
    }
}
