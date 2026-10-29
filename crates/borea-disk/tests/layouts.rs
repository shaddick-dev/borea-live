#![allow(clippy::unwrap_used)]
#[path = "../../../backend/tests/common.rs"]
mod common;
use borea_core::*;
use borea_disk::*;
fn facts(p: &Plan) -> DiskFacts {
    DiskFacts {
        uefi: true,
        disks: vec![Disk {
            id: p.disk.disk().clone(),
            readonly: false,
            in_use: false,
            live: false,
            removable: false,
            transport: "virtio".into(),
            parts: Vec::new(),
        }],
    }
}
#[test]
fn four_distros_have_encrypted_uefi_layouts() {
    for d in Distribution::ALL {
        let p = common::plan(d, true);
        let f = facts(&p);
        let l = layout(&p.disk, &f, d, true).unwrap();
        assert!(matches!(l.changes[0], Change::Wipe));
        assert!(
            matches!(l.changes.last().unwrap(),Change::Create{type_code,..} if type_code=="8309")
        );
        assert_eq!(l.boot.is_some(), matches!(d, Distribution::Debian | Distribution::Fedora));
        assert_eq!(l.subvolumes[0].1, "/");
    }
}
#[test]
fn reject_live_mounted_readonly_and_changed_disks() {
    let p = common::plan(Distribution::Arch, true);
    for flag in 0..4 {
        let mut f = facts(&p);
        match flag {
            0 => f.disks[0].live = true,
            1 => f.disks[0].in_use = true,
            2 => f.disks[0].readonly = true,
            _ => f.disks[0].id.serial = Some("REPLACED".into()),
        }
        assert!(validate(&p.disk, &f, true).is_err());
    }
}
#[test]
fn four_kilobyte_sector_layout_is_aligned() {
    let mut p = common::plan(Distribution::Arch, true);
    if let DiskPlan::Whole { disk } = &mut p.disk {
        disk.logical_sector = 4096;
    }
    let f = facts(&p);
    let l = layout(&p.disk, &f, Distribution::Arch, true).unwrap();
    for c in l.changes {
        if let Change::Create { start_sector, end_sector, .. } = c {
            assert_eq!(start_sector * 4096 % (1024 * 1024), 0);
            assert!(end_sector * 4096 < f.disks[0].id.size);
        }
    }
}
#[test]
fn alongside_preserves_windows_and_refuses_unsafe_ntfs() {
    let mut p = common::plan(Distribution::Debian, true);
    let mut f = facts(&p);
    let w = Partition {
        path: "/dev/vda3".into(),
        number: 3,
        uuid: "WINDOWS".into(),
        start: 3 * GIB,
        size: 90 * GIB,
        fs: "ntfs".into(),
        efi: false,
        free: None,
        bitlocker: false,
        ntfs_checked: true,
        ntfs_hibernated: false,
        ntfs_dirty: false,
        ntfs_min_size: Some(10 * GIB),
    };
    let mut esp = w.clone();
    assert_eq!(w.max_linux_size(), Some(50 * GIB));
    esp.path = "/dev/vda1".into();
    esp.number = 1;
    esp.efi = true;
    esp.fs = "vfat".into();
    esp.size = GIB;
    esp.free = Some(500 * 1024 * 1024);
    f.disks[0].parts = vec![esp, w.clone()];
    p.disk = DiskPlan::Alongside {
        disk: p.disk.disk().clone(),
        windows_part: PartId { path: w.path, uuid: w.uuid, number: 3 },
        linux_size: 45 * GIB,
    };
    let l = layout(&p.disk, &f, p.distro, true).unwrap();
    assert!(!l.changes.iter().any(|c| matches!(c, Change::Wipe)));
    assert!(l.changes.iter().any(|c| matches!(c, Change::ResizeNtfs { number: 3, .. })));
    let mut small_esp = f.clone();
    small_esp.disks[0].parts[0].free = Some(50 * 1024 * 1024);
    for distro in [Distribution::Arch, Distribution::OpenSuse] {
        let layout = layout(&p.disk, &small_esp, distro, true).unwrap();
        assert_eq!(layout.boot_fs, "vfat");
        assert_eq!(layout.esp_mount, "/efi");
        assert!(layout.changes.iter().any(
            |change| matches!(change, Change::Create { type_code, .. } if type_code == "ea00")
        ));
    }
    small_esp.disks[0].parts[0].free = Some(31 * 1024 * 1024);
    assert!(layout(&p.disk, &small_esp, p.distro, true).is_err());
    for flag in 0..3 {
        let mut f = f.clone();
        match flag {
            0 => f.disks[0].parts[1].bitlocker = true,
            1 => f.disks[0].parts[1].ntfs_hibernated = true,
            _ => f.disks[0].parts[1].ntfs_checked = false,
        }
        assert!(layout(&p.disk, &f, p.distro, true).is_err());
        assert_eq!(f.disks[0].parts[1].max_linux_size(), None);
    }
}
#[test]
fn null_lsblk_fields_do_not_invent_identity() {
    let f=parse_lsblk(r#"{"blockdevices":[{"type":"disk","path":"/dev/vda","size":99999,"serial":null,"mountpoints":[null]}]}"#,true).unwrap();
    assert_eq!(f.disks[0].id.serial, None);
    assert!(!f.disks[0].in_use);
}

#[test]
fn active_unmounted_mappings_are_not_free_disks() {
    let facts=parse_lsblk(r#"{"blockdevices":[{"type":"disk","path":"/dev/vda","size":99999,"serial":"test","mountpoints":[null],"children":[{"type":"part","path":"/dev/vda1","mountpoints":[null],"children":[{"type":"crypt","path":"/dev/mapper/open-volume","mountpoints":[null]}]}]}]}"#,true).unwrap();
    assert!(facts.disks[0].in_use);
}

#[test]
fn handing_off_changes_nothing_for_a_whole_disk_and_only_shrinks_windows_alongside() {
    let mut p = common::plan(Distribution::Arch, true);
    let mut f = facts(&p);
    assert_eq!(make_space(&p.disk, &f, 20 * GIB).unwrap(), Vec::<Change>::new());
    f.disks[0].live = true;
    assert!(make_space(&p.disk, &f, 20 * GIB).is_err(), "never the live medium");
    f.disks[0].live = false;

    let w = Partition {
        path: "/dev/vda3".into(),
        number: 3,
        uuid: "WINDOWS".into(),
        start: 3 * GIB,
        size: 90 * GIB,
        fs: "ntfs".into(),
        efi: false,
        free: None,
        bitlocker: false,
        ntfs_checked: true,
        ntfs_hibernated: false,
        ntfs_dirty: false,
        ntfs_min_size: Some(10 * GIB),
    };
    f.disks[0].parts = vec![w.clone()];
    p.disk = DiskPlan::Alongside {
        disk: p.disk.disk().clone(),
        windows_part: PartId { path: w.path.clone(), uuid: w.uuid.clone(), number: 3 },
        linux_size: 45 * GIB,
    };
    // No ESP is needed beside Windows: the other installer makes its own.
    let changes = make_space(&p.disk, &f, 20 * GIB).unwrap();
    assert_eq!(changes.len(), 1);
    assert!(
        matches!(changes[0], Change::ResizeNtfs { number: 3, new_size, .. } if new_size == 45 * GIB),
        "{changes:?}"
    );
    assert!(make_space(&p.disk, &f, 50 * GIB).is_err(), "too little for the system");

    for flag in 0..4 {
        let mut f = f.clone();
        match flag {
            0 => f.disks[0].parts[0].bitlocker = true,
            1 => f.disks[0].parts[0].ntfs_hibernated = true,
            2 => f.disks[0].parts[0].ntfs_dirty = true,
            _ => f.disks[0].parts[0].ntfs_min_size = Some(30 * GIB),
        }
        assert!(make_space(&p.disk, &f, 20 * GIB).is_err(), "case {flag}");
    }
    // A partition inside the space Windows gives up would be overwritten.
    let mut crowded = f.clone();
    let mut recovery = w.clone();
    recovery.path = "/dev/vda4".into();
    recovery.number = 4;
    recovery.start = 80 * GIB;
    recovery.size = GIB;
    crowded.disks[0].parts.push(recovery);
    assert!(make_space(&p.disk, &crowded, 20 * GIB).is_err());
}
