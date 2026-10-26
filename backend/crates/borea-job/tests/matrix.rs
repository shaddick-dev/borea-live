#![allow(clippy::unwrap_used)]
#[path = "../../../tests/common.rs"]
mod common;
use borea_backend::*;
use borea_core::*;
use borea_disk::*;
use borea_job::*;
#[test]
fn every_distro_profile_hardware_path_compiles_and_review_matches_job() {
    let mut n = 0;
    for (_, hw) in borea_catalog::hardware::samples() {
        for d in Distribution::ALL {
            for env in ["workstation", "server", "minimal"] {
                for alongside in [false, true] {
                    let mut p = common::plan(d, env == "workstation");
                    p.env = env.into();
                    let mut disk = Disk {
                        id: p.disk.disk().clone(),
                        readonly: false,
                        in_use: false,
                        live: false,
                        removable: false,
                        transport: "virtio".into(),
                        parts: Vec::new(),
                    };
                    if alongside {
                        disk.parts = vec![
                            Partition {
                                path: "/dev/vda1".into(),
                                number: 1,
                                uuid: "EFI".into(),
                                start: 1024 * 1024,
                                size: GIB,
                                fs: "vfat".into(),
                                efi: true,
                                free: Some(GIB / 2),
                                bitlocker: false,
                                ntfs_checked: false,
                                ntfs_hibernated: false,
                                ntfs_dirty: false,
                                ntfs_min_size: None,
                            },
                            Partition {
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
                            },
                        ];
                        p.disk = DiskPlan::Alongside {
                            disk: p.disk.disk().clone(),
                            windows_part: PartId {
                                path: "/dev/vda3".into(),
                                uuid: "WINDOWS".into(),
                                number: 3,
                            },
                            linux_size: 45 * GIB,
                        };
                    }
                    let b = Bootstrap {
                        distro: d,
                        release: p.release.clone(),
                        artifact: if d == Distribution::Debian {
                            None
                        } else {
                            Some(Artifact {
                                checksum: None,
                                url: format!("https://example.org/{}.tar.xz", d.key()),
                                sha256: "a".repeat(64),
                                signature_url: "https://example.org/image.sig".into(),
                                keyring: format!("/usr/share/keyrings/borea/{}.gpg", d.key()),
                            })
                        },
                    };
                    let job =
                        compile(&p, &hw, &DiskFacts { uefi: true, disks: vec![disk] }, &b).unwrap();
                    assert_eq!(job.stages.iter().map(|s| s.weight).sum::<u32>(), 100);
                    let wire = serde_json::to_string(&job).unwrap();
                    assert_eq!(job, serde_json::from_str::<Job>(&wire).unwrap());
                    assert_eq!(job.digest().unwrap().len(), 64);
                    let base = borea_core::review(&p, &hw).unwrap();
                    for pkg in base.packages {
                        assert!(job.review.packages.contains(&pkg));
                    }
                    let script = job.render_script();
                    assert!(script.contains("cryptsetup"));
                    assert!(script.contains("<redacted secret>"));
                    assert!(!script.contains("--nogpgcheck"));
                    assert!(!script.contains("--allow-unauthenticated"));
                    n += 1;
                }
            }
        }
    }
    assert_eq!(n, 120);
}
#[test]
fn render_escapes_shell_metacharacters() {
    assert_eq!(quote("x'$(touch /tmp/x)"), "'x'\\''$(touch /tmp/x)'");
}
