#![allow(clippy::unwrap_used)]
#[path = "../../../tests/common.rs"]
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
    for flag in 0..3 {
        let mut f = f.clone();
        match flag {
            0 => f.disks[0].parts[1].bitlocker = true,
            1 => f.disks[0].parts[1].ntfs_hibernated = true,
            _ => f.disks[0].parts[1].ntfs_checked = false,
        }
        assert!(layout(&p.disk, &f, p.distro, true).is_err());
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
