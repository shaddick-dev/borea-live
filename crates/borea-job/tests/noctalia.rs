#![allow(clippy::unwrap_used)]
#[path = "../../../backend/tests/common.rs"]
mod common;
use borea_core::*;
use borea_disk::*;
use borea_job::*;

fn scenario(alongside: bool) -> Job {
    scenario_with_locale(alongside, common::plan(Distribution::Arch, true).locale)
}

fn scenario_with_locale(alongside: bool, locale: LocaleChoice) -> Job {
    scenario_target(alongside, locale, false)
}
fn scenario_target(alongside: bool, locale: LocaleChoice, cachyos: bool) -> Job {
    let mut plan = common::plan(Distribution::Arch, true);
    plan.locale = locale;
    plan.derivative = cachyos.then_some(ArchDerivative::Cachyos);
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

fn written<'a>(job: &'a Job, name: &str) -> &'a str {
    job.stages
        .iter()
        .flat_map(|s| &s.actions)
        .find_map(|action| match action {
            Action::WriteFile { path, contents, .. } if path == name => Some(contents.as_str()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn region_choices_reach_shell_greeter_disk_unlock_and_system_for_both_layouts() {
    // Deliberately mix language, keyboard and zone: none should be inferred
    // from another choice, or from the build/live environment.
    for (lang, layout, variant, console, timezone) in [
        ("fr_FR.UTF-8", "de", "nodeadkeys", "de-latin1", "Asia/Tokyo"),
        ("pt_BR.UTF-8", "us", "intl", "us", "America/Sao_Paulo"),
        ("en_GB.UTF-8", "gb", "", "uk", "Europe/London"),
    ] {
        for alongside in [false, true] {
            let job = scenario_with_locale(
                alongside,
                LocaleChoice {
                    lang: lang.into(),
                    timezone: timezone.into(),
                    keyboard: Keyboard {
                        layout: layout.into(),
                        variant: variant.into(),
                        console: console.into(),
                    },
                },
            );
            assert_eq!(written(&job, "/etc/locale.conf"), format!("LANG={lang}\n"));
            assert_eq!(written(&job, "/etc/locale.gen"), format!("{lang} UTF-8\n"));
            assert_eq!(written(&job, "/etc/vconsole.conf"), format!("KEYMAP={console}\n"));
            let shell: toml::Value =
                toml::from_str(written(&job, "/etc/skel/.config/noctalia/config.toml")).unwrap();
            assert_eq!(shell["shell"]["lang"].as_str(), Some(lang));
            assert!(shell.get("keybinds").is_none(), "retain Noctalia's built-in keys");
            let greeter: toml::Value =
                toml::from_str(written(&job, "/var/lib/noctalia-greeter/greeter.toml")).unwrap();
            assert_eq!(greeter["keyboard"]["layout"].as_str(), Some(layout));
            assert_eq!(greeter["keyboard"]["variant"].as_str(), Some(variant));
            let hyprland = written(&job, "/etc/skel/.config/hypr/hyprland.lua");
            assert!(
                hyprland.contains(&format!("kb_layout = \"{layout}\", kb_variant = \"{variant}\""))
            );
            let actions: Vec<_> = job.stages.iter().flat_map(|s| &s.actions).collect();
            assert!(actions.iter().any(|a| matches!(a, Action::Run(c)
                if c.root == Root::Target && c.program == "/usr/bin/ln"
                && c.args == ["-sf", &format!("/usr/share/zoneinfo/{timezone}"), "/etc/localtime"])));
            let useradd = actions
                .iter()
                .position(|a| matches!(a, Action::Run(c) if c.program == "/usr/sbin/useradd"))
                .unwrap();
            for path in
                ["/etc/skel/.config/noctalia/config.toml", "/etc/skel/.config/hypr/hyprland.lua"]
            {
                assert!(
                    actions
                        .iter()
                        .position(|a| matches!(a, Action::WriteFile { path: p, .. } if p == path))
                        .unwrap()
                        < useradd
                );
            }
        }
    }
}

#[test]
fn noctalia_uses_upstream_shortcuts_without_borea_remaps() {
    let job = scenario(false);
    let config = written(&job, "/etc/skel/.config/hypr/hyprland.lua");
    for (key, command) in [
        ("SUPER + SPACE", "panel-toggle launcher"),
        ("SUPER + S", "panel-toggle control-center"),
        ("SUPER + comma", "settings-toggle"),
        ("ALT + Tab", "window-switcher hold"),
        ("XF86AudioRaiseVolume", "volume-up"),
        ("XF86AudioLowerVolume", "volume-down"),
        ("XF86AudioMute", "volume-mute"),
        ("XF86MonBrightnessUp", "brightness-up"),
        ("XF86MonBrightnessDown", "brightness-down"),
    ] {
        assert!(
            config.contains(&format!(
                "hl.bind(\"{key}\", hl.dsp.exec_cmd(\"noctalia msg {command}\"))"
            ))
        );
    }
    for old in [
        "SUPER + RETURN",
        "SUPER + SHIFT + Q",
        "SUPER + SHIFT + E",
        "SUPER + SHIFT + S",
        "SUPER + L",
    ] {
        assert!(!config.contains(old), "unexpected Borea shortcut: {old}");
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

#[test]
fn cachyos_both_layouts_use_signed_upstream_packages_and_the_correct_kernel() {
    for alongside in [false, true] {
        let job = scenario_target(alongside, common::plan(Distribution::Arch, true).locale, true);
        assert_eq!(job.review.derivative, Some(ArchDerivative::Cachyos));
        assert!(job.stages[0].actions.iter().all(Action::preparation_only));
        let names = job.review.packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>();
        for package in borea_backend::cachyos::PACKAGES {
            assert!(names.contains(package), "{package}");
        }
        assert!(!names.contains(&"linux"));
        let boot = written(&job, "/boot/loader/entries/arch.conf");
        assert!(boot.contains("title CachyOS\n"));
        assert!(boot.contains("linux /vmlinuz-linux-cachyos\n"));
        assert!(boot.contains("initrd /initramfs-linux-cachyos.img\n"));
        assert!(
            job.stages
                .iter()
                .flat_map(|s| &s.actions)
                .any(|a| matches!(a, Action::CopyArchRepositories))
        );
        let config = written(&job, "/etc/pacman.conf");
        assert!(config.find("[cachyos]").unwrap() < config.find("[core]").unwrap());
        assert!(config.contains("SigLevel = Required"));
        let queue = written(&job, "/var/lib/borea/firstboot.json");
        assert!(queue.contains("firefox"));
        for required in borea_backend::cachyos::PACKAGES {
            assert!(!queue.contains(required), "mandatory package deferred: {required}");
        }
        assert!(job.stages.iter().flat_map(|s|&s.actions).any(|a|matches!(a,Action::Run(c)if c.program=="/usr/share/libalpm/scripts/cachyos-branding" && c.args==["os-release"])));
        assert!(names.contains(&"mesa"));
        assert!(names.contains(&"vulkan-intel"));
    }
}
#[test]
fn cachyos_secure_boot_uses_its_kernel_preset_for_both_layouts() {
    for alongside in [false, true] {
        let prior = scenario_target(alongside, common::plan(Distribution::Arch, true).locale, true);
        let mut plan = prior.plan;
        plan.secure_boot = Some(true);
        let mut hw = borea_catalog::hardware::samples().remove(0).1;
        hw.secure_boot = false;
        hw.setup_mode = Some(true);
        let mut facts = DiskFacts {
            uefi: true,
            disks: vec![Disk {
                id: plan.disk.disk().clone(),
                readonly: false,
                in_use: false,
                live: false,
                removable: false,
                transport: "virtio".into(),
                parts: vec![],
            }],
        };
        // Reuse the existing layout's preserved NTFS fixture when needed.
        if alongside {
            facts.disks[0].parts = vec![
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
        }
        let job = compile(&plan, &hw, &facts, &prior.bootstrap).unwrap();
        let preset = written(&job, "/etc/mkinitcpio.d/linux-cachyos.preset");
        assert!(preset.contains("ALL_kver=\"/boot/vmlinuz-linux-cachyos\""));
        assert!(!job.stages.iter().flat_map(|s| &s.actions).any(
            |a| matches!(a,Action::WriteFile{path,..}if path=="/etc/mkinitcpio.d/linux.preset")
        ));
        assert!(
            written(&job, "/etc/pacman.d/hooks/zz-borea-secureboot.hook")
                .contains("Target = linux-cachyos\n")
        );
    }
}
