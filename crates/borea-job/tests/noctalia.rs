#![allow(clippy::unwrap_used)]
#[path = "../../../backend/tests/common.rs"]
mod common;
use borea_core::*;
use borea_disk::*;
use borea_job::*;

fn scenario(alongside: bool) -> Job {
    let mut plan = common::plan(Distribution::Arch, true);
    plan.desktop = Some(Desktop::HyprlandNoctalia);
    plan.env = "composed".into();
    plan.secure_boot = Some(false);
    plan.extras = ["gaming", "printing", "codecs", "flatpak"].map(str::to_owned).into();
    plan.apps.insert("firefox".into());
    let mut disk = Disk {
        id: plan.disk.disk().clone(),
        readonly: false,
        in_use: false,
        live: false,
        removable: false,
        transport: "virtio".into(),
        parts: vec![],
    };
    if alongside {
        disk.parts = vec![
            Partition {
                path: "/dev/vda1".into(),
                number: 1,
                uuid: "EFI".into(),
                start: GIB,
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
        plan.disk = DiskPlan::Alongside {
            disk: disk.id.clone(),
            windows_part: PartId { path: "/dev/vda3".into(), number: 3, uuid: "WINDOWS".into() },
            linux_size: 45 * GIB,
        };
    }
    let bootstrap = borea_backend::Bootstrap {
        distro: Distribution::Arch,
        release: "rolling".into(),
        artifact: Some(borea_backend::Artifact {
            url: "https://example.org/arch.tar.zst".into(),
            sha256: "a".repeat(64),
            signature_url: "https://example.org/arch.tar.zst.sig".into(),
            keyring: "/usr/share/keyrings/borea/arch.gpg".into(),
            checksum: None,
        }),
    };
    let mut hw = borea_catalog::hardware::samples().remove(0).1;
    hw.secure_boot = false;
    hw.gpus =
        vec![borea_catalog::hardware::Gpu::new("0000:03:00.0", 0x8086, 0x56a0, "Intel Arc A770")];
    compile(&plan, &hw, &DiskFacts { uefi: true, disks: vec![disk] }, &bootstrap).unwrap()
}

#[test]
fn both_layouts_have_graphical_login_and_defer_only_optional_packages() {
    for alongside in [false, true] {
        let job = scenario(alongside);
        assert_eq!(job.stages.iter().map(|s| s.weight).sum::<u32>(), 100);
        assert!(job.stages[0].actions.iter().all(Action::preparation_only));
        let script = job.render_script();
        for name in ["mesa", "vulkan-intel", "intel-media-driver", "linux-firmware"] {
            assert!(job.review.packages.iter().any(|p| p.name == name), "{name}");
        }
        assert!(script.contains("noctalia-greeter-session"));
        assert!(script.contains("Exec=/usr/bin/start-hyprland"));
        assert!(script.contains("hyprland.lua"));
        assert!(script.contains("hl.on(\"hyprland.start\""));
        assert!(script.contains("noctalia msg session lock"));
        assert!(script.contains("lock_before_suspend = true"));
        assert!(script.contains("--after-login tester"));
        assert!(script.contains("plymouth autodetect"));
        assert!(script.contains("'graphical.target'"));
        let actions = job.stages.iter().flat_map(|s| &s.actions).collect::<Vec<_>>();
        let queue = actions
            .iter()
            .find_map(|a| match a {
                Action::WriteFile { path, contents, .. }
                    if path == "/var/lib/borea/firstboot.json" =>
                {
                    Some(contents)
                }
                _ => None,
            })
            .unwrap();
        let queue: serde_json::Value = serde_json::from_str(queue).unwrap();
        let args = queue[0]["args"].as_array().unwrap();
        for optional in ["steam", "ffmpeg", "cups", "firefox"] {
            assert!(args.iter().any(|v| v == optional), "{optional}");
        }
        for critical in
            ["hyprland", "noctalia", "noctalia-greeter", "mesa", "vulkan-intel", "linux"]
        {
            assert!(!args.iter().any(|v| v == critical), "{critical}");
        }
        for action in &actions {
            if let Action::Run(command) = action {
                if command.program == "/usr/bin/pacman" && command.root == Root::Target {
                    assert!(
                        !command
                            .args
                            .iter()
                            .any(|p| ["steam", "firefox", "ffmpeg", "cups"].contains(&p.as_str()))
                    );
                }
            }
        }
        if alongside {
            assert!(!script.contains("--zap-all"));
            assert!(!actions.iter().any(|a|matches!(a,Action::Run(c) if c.program == "/usr/sbin/mkfs.fat" && c.args.iter().any(|p|p == "/dev/vda1"))));
        }
        let mut changed = job.clone();
        changed.external.noctalia = Some("5.4.1".into());
        assert_ne!(job.digest().unwrap(), changed.digest().unwrap());
    }
}

#[test]
fn no_optional_apps_or_extras_are_added_implicitly() {
    let mut job = scenario(false);
    job.plan.extras.clear();
    job.plan.apps.clear();
    let review =
        borea_core::review(&job.plan, &borea_catalog::hardware::samples().remove(0).1).unwrap();
    for name in ["steam", "cups", "ffmpeg", "firefox", "flatpak"] {
        assert!(!review.packages.iter().any(|p| p.name == name));
    }
    for name in ["hyprland", "noctalia", "noctalia-greeter", "greetd", "plymouth"] {
        assert!(review.packages.iter().any(|p| p.name == name));
    }
    job.plan.distro = Distribution::Fedora;
    assert!(job.plan.validate_intent().is_err());
}

#[test]
fn gaming_enables_multilib_for_selected_apps_even_without_a_detected_gpu() {
    let mut job = scenario(false);
    job.plan.apps.insert("steam".into());
    let mut hw = borea_catalog::hardware::samples().remove(0).1;
    hw.gpus.clear();
    let review = borea_core::review(&job.plan, &hw).unwrap();
    assert!(review.packages.iter().any(|p| p.name == "steam" && p.source == "arch.multilib"));
}
