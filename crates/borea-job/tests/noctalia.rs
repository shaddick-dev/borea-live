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
    scenario_desktop(alongside, locale, cachyos, Desktop::HyprlandNoctalia)
}
fn scenario_desktop(alongside: bool, locale: LocaleChoice, cachyos: bool, desktop: Desktop) -> Job {
    scenario_apps(alongside, locale, cachyos, desktop, false)
}
fn scenario_apps(
    alongside: bool,
    locale: LocaleChoice,
    cachyos: bool,
    desktop: Desktop,
    aur: bool,
) -> Job {
    let mut plan = common::plan(Distribution::Arch, true);
    plan.locale = locale;
    plan.derivative = cachyos.then_some(ArchDerivative::Cachyos);
    plan.desktop = Some(desktop);
    plan.env = "composed".into();
    plan.secure_boot = Some(false);
    plan.extras = ["gaming", "printing", "codecs", "flatpak"].map(str::to_owned).into();
    plan.apps.insert("firefox".into());
    if aur {
        plan.apps.insert("vesktop".into());
        plan.prefer = SourcePreference::Aur;
    }
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
fn every_compositor_follows_noctalias_guide_with_the_same_window_keys() {
    let locale = common::plan(Distribution::Arch, true).locale;
    let config = |desktop| {
        let job = scenario_desktop(false, locale.clone(), false, desktop);
        let path = match desktop {
            Desktop::HyprlandNoctalia => "/etc/skel/.config/hypr/hyprland.lua",
            Desktop::NiriNoctalia => "/etc/skel/.config/niri/config.kdl",
            _ => "/etc/skel/.config/mango/config.conf",
        };
        (
            written(&job, path).to_owned(),
            written(&job, "/etc/skel/.config/noctalia/config.toml").to_owned(),
        )
    };
    let (hyprland, hyprland_shell) = config(Desktop::HyprlandNoctalia);
    let (niri, _) = config(Desktop::NiriNoctalia);
    let (mango, mango_shell) = config(Desktop::MangoNoctalia);
    // Noctalia's own sections: autostart, appearance, rules, blur, its keys.
    for part in [
        "hl.exec_cmd(\"noctalia\")",
        "rounding = 20",
        "persistent = true",
        "dev.noctalia.Noctalia",
        "blur_popups = true",
        "\"panel-toggle launcher\"",
        "\"window-switcher hold\"",
    ] {
        assert!(hyprland.contains(part), "hyprland: {part}");
    }
    for part in [
        "spawn-at-startup \"noctalia\"",
        "geometry-corner-radius 20",
        "dev.noctalia.Noctalia",
        "honor-xdg-activation-with-invalid-serial",
        "^noctalia-wallpaper",
        "background-effect",
        "noctalia msg panel-toggle launcher",
        "noctalia msg window-switcher hold",
    ] {
        assert!(niri.contains(part), "niri: {part}");
    }
    for part in [
        "exec_once=noctalia",
        "blur=1",
        "layer_shadows=1",
        "bind=SUPER,space,spawn,noctalia msg panel-toggle launcher",
    ] {
        assert!(mango.contains(part), "mango: {part}");
    }
    // Mango draws the shadows, so Noctalia's are off there only.
    assert!(mango_shell.contains("[shell.panel]\nshadow = false"));
    assert!(!hyprland_shell.contains("shadow = false"));
    // The same small set of window keys everywhere, with the installed terminal.
    for (config, terminal, close) in [
        (
            &hyprland,
            "\"SUPER + T\", hl.dsp.exec_cmd(\"foot\")",
            "\"SUPER + Q\", hl.dsp.window.close()",
        ),
        (&niri, "Mod+T { spawn \"foot\"; }", "Mod+Q repeat=false { close-window; }"),
        (&mango, "bind=SUPER,t,spawn,foot", "bind=SUPER,q,killclient,"),
    ] {
        assert!(config.contains(terminal), "{terminal}");
        assert!(config.contains(close), "{close}");
    }
    // None of the compositors' own example configurations.
    assert!(!niri.contains("org.wezfurlong.wezterm"));
    assert!(!mango.contains("rofi"));
    assert!(!hyprland.contains("togglesplit"));
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
fn cachyos_is_named_as_itself_while_installing() {
    for (cachyos, name) in [(false, "Installing Arch Linux."), (true, "Installing CachyOS.")] {
        let job = scenario_target(false, common::plan(Distribution::Arch, true).locale, cachyos);
        assert!(job.stages.iter().any(|s| s.title == name), "{name}");
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

#[test]
fn every_noctalia_compositor_has_its_own_packages_session_and_region_for_both_distros_and_layouts()
{
    for desktop in [Desktop::HyprlandNoctalia, Desktop::NiriNoctalia, Desktop::MangoNoctalia] {
        let recipe = desktop.noctalia().unwrap();
        for cachyos in [false, true] {
            for alongside in [false, true] {
                let mut locale = common::plan(Distribution::Arch, true).locale;
                locale.lang = "fr_FR.UTF-8".into();
                locale.timezone = "Europe/Paris".into();
                locale.keyboard.layout = "de".into();
                locale.keyboard.variant = "nodeadkeys".into();
                locale.keyboard.console = "de-latin1".into();
                let job = scenario_desktop(alongside, locale, cachyos, desktop);
                let packages =
                    job.review.packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>();
                assert!(packages.contains(&recipe.package));
                for other in borea_catalog::noctalia::RECIPES {
                    if other.compositor != recipe.compositor {
                        assert!(!packages.contains(&other.package));
                    }
                }
                for package in recipe.packages {
                    assert!(packages.contains(package));
                }
                let session = written(
                    &job,
                    &format!("/usr/share/wayland-sessions/borea-{}.desktop", recipe.compositor),
                );
                assert!(session.contains(recipe.session));
                assert!(
                    !session.contains("dbus-run-session"),
                    "retain the login's user bus for portals"
                );
                assert!(session.contains(&format!("DesktopNames={}\n", recipe.desktop_name)));
                assert!(
                    written(&job, "/etc/greetd/config.toml").contains("noctalia-greeter-session")
                );
                assert!(
                    written(&job, "/var/lib/noctalia-greeter/greeter.toml")
                        .contains("variant = \"nodeadkeys\"")
                );
                assert!(
                    written(&job, "/etc/skel/.config/noctalia/config.toml")
                        .contains("lang = \"fr_FR.UTF-8\"")
                );
                assert!(
                    written(&job, "/etc/skel/.config/xdg-desktop-portal/portals.conf")
                        .contains(&format!("ScreenCast={}", recipe.portal))
                );
                let (name, path) = match desktop {
                    Desktop::HyprlandNoctalia => {
                        ("hyprland.lua", "/etc/skel/.config/hypr/hyprland.lua")
                    }
                    Desktop::NiriNoctalia => ("niri.kdl", "/etc/skel/.config/niri/config.kdl"),
                    Desktop::MangoNoctalia => ("mango.conf", "/etc/skel/.config/mango/config.conf"),
                    _ => unreachable!(),
                };
                let config = written(&job, path);
                assert!(config.contains("nodeadkeys"));
                assert!(config.contains("noctalia"));
                assert!(!config.contains("@@"));
                assert!(written(&job, "/var/lib/borea/firstboot.json").contains("firefox"));
                assert_eq!(job.plan.desktop, Some(desktop));
                let approved = borea_backend::external::resolve(&job.review).unwrap();
                assert_eq!(approved.noctalia.as_deref(), Some("5"));
                assert!(
                    borea_backend::noctalia::check_version(
                        "5.4.0-1",
                        approved.noctalia.as_deref().unwrap()
                    )
                    .is_ok()
                );
                if let Ok(dir) = std::env::var("BOREA_COMPOSITOR_CONFIGS") {
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(std::path::Path::new(&dir).join(name), config).unwrap();
                    std::fs::write(
                        std::path::Path::new(&dir).join("noctalia.toml"),
                        written(&job, "/etc/skel/.config/noctalia/config.toml"),
                    )
                    .unwrap();
                }
            }
        }
    }
}

#[test]
fn aur_apps_are_reviewed_and_queued_after_login_on_all_six_desktops_and_both_layouts() {
    for desktop in [Desktop::HyprlandNoctalia, Desktop::NiriNoctalia, Desktop::MangoNoctalia] {
        for cachyos in [false, true] {
            for alongside in [false, true] {
                let job = scenario_apps(
                    alongside,
                    common::plan(Distribution::Arch, true).locale,
                    cachyos,
                    desktop,
                    true,
                );
                assert!(
                    job.review
                        .packages
                        .iter()
                        .any(|p| p.source == "aur" && p.name == "vesktop-bin")
                );
                assert!(
                    !job.review
                        .packages
                        .iter()
                        .any(|p| p.source == "flathub" && p.name == "dev.vencord.Vesktop")
                );
                assert!(job.stages.iter().flat_map(|s|&s.actions).any(|a|matches!(a,Action::DeferAur{packages} if packages.contains(&"vesktop-bin".to_owned()))));
                assert!(
                    !job.stages
                        .iter()
                        .flat_map(|s| &s.actions)
                        .any(|a| matches!(a, Action::BuildAur { .. }))
                );
                let queue: serde_json::Value =
                    serde_json::from_str(written(&job, "/var/lib/borea/firstboot.json")).unwrap();
                let queue = queue.as_array().unwrap();
                let build = queue
                    .iter()
                    .position(|a| {
                        a["program"] == "/usr/bin/borea-firstboot"
                            && a["args"] == serde_json::json!(["--aur"])
                    })
                    .unwrap();
                assert!(build > 0 && queue[0]["program"] == "/usr/bin/pacman");
                assert!(
                    written(&job, "/etc/systemd/system/borea-firstboot.service.d/login.conf")
                        .contains("--after-login")
                );
            }
        }
    }
}

fn tumbleweed(desktop: Desktop, alongside: bool) -> Job {
    let mut plan = common::plan(Distribution::OpenSuse, true);
    plan.desktop = Some(desktop);
    plan.env = "composed".into();
    plan.secure_boot = Some(false);
    plan.extras = ["gaming", "printing", "codecs", "flatpak"].map(str::to_owned).into();
    plan.apps.insert("firefox".into());
    let reference = scenario_desktop(alongside, plan.locale.clone(), false, desktop);
    plan.disk = reference.plan.disk.clone();
    let disk = Disk {
        id: plan.disk.disk().clone(),
        readonly: false,
        in_use: false,
        live: false,
        removable: false,
        transport: "virtio".into(),
        parts: if alongside {
            vec![
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
            ]
        } else {
            vec![]
        },
    };
    let bootstrap = borea_backend::Bootstrap {
        distro: Distribution::OpenSuse,
        release: "tumbleweed".into(),
        artifact: Some(borea_backend::Artifact {
            url: "https://example.org/tumbleweed.tar.xz".into(),
            sha256: "a".repeat(64),
            signature_url: "https://example.org/tumbleweed.sig".into(),
            keyring: "/usr/share/keyrings/borea/opensuse.gpg".into(),
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
fn tumbleweed_installs_every_noctalia_compositor_from_its_own_packages_for_both_layouts() {
    for desktop in [Desktop::HyprlandNoctalia, Desktop::NiriNoctalia, Desktop::MangoNoctalia] {
        let recipe = desktop.noctalia().unwrap();
        for alongside in [false, true] {
            let job = tumbleweed(desktop, alongside);
            let has = |source: &str, name: &str| {
                job.review.packages.iter().any(|p| p.source == source && p.name == name)
            };
            let (source, package) = recipe.compositor_package(Distribution::OpenSuse).unwrap();
            assert!(has(source, package), "{package} from {source}");
            for other in borea_catalog::noctalia::RECIPES {
                if other.compositor != recipe.compositor {
                    let (_, name) = other.compositor_package(Distribution::OpenSuse).unwrap();
                    assert!(!job.review.packages.iter().any(|p| p.name == name));
                }
            }
            for name in borea_catalog::noctalia::base_packages(Distribution::OpenSuse) {
                assert!(has("opensuse.oss", name), "{name}");
            }
            for name in recipe.packages {
                assert!(has("opensuse.oss", name), "{name}");
            }
            // Arch's names never leak into a Tumbleweed install.
            for name in ["pipewire-pulse", "ttf-dejavu", "mangowm", "lib32-gamemode"] {
                assert!(!job.review.packages.iter().any(|p| p.name == name), "{name}");
            }
            assert!(has("opensuse.non-oss", "steam"));
            assert!(has("opensuse.packman", "gstreamer-plugins-bad-codecs"));
            assert!(has("opensuse.oss", "cups"));
            let mango = job.review.packages.iter().any(|p| p.source == "opensuse.mangowc");
            assert_eq!(mango, desktop == Desktop::MangoNoctalia);
            assert!(
                job.stages
                    .iter()
                    .all(|s| s.actions.iter().all(|a| !matches!(a, Action::DeferAur { .. })))
            );
            assert!(written(&job, "/etc/greetd/config.toml").contains("noctalia-greeter-session"));
            let session = written(
                &job,
                &format!("/usr/share/wayland-sessions/borea-{}.desktop", recipe.compositor),
            );
            assert!(session.contains(recipe.session));
            assert!(
                written(&job, "/etc/dracut.conf.d/90-borea.conf")
                    .contains("hostonly_cmdline=\"no\"")
            );
            assert!(
                written(&job, "/etc/kernel/cmdline")
                    .contains("rd.luks.name=${LUKS_UUID}=borea-root")
            );
            assert!(written(&job, "/etc/kernel/cmdline").contains("splash"));
            let sdbootutil = job
                .stages
                .iter()
                .flat_map(|s| &s.actions)
                .filter_map(|a| match a {
                    Action::Run(c) if c.program == "/usr/bin/sdbootutil" => Some(c.args.join(" ")),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(sdbootutil.len(), 2);
            for args in &sdbootutil {
                assert!(
                    args.contains("--rootfs device --rootfs-data /dev/mapper/borea-root"),
                    "{args}"
                );
            }
            let approved = borea_backend::external::resolve(&job.review).unwrap();
            assert_eq!(approved.noctalia.as_deref(), Some("5"));
            let script = job.render_script();
            assert_eq!(script.contains(borea_backend::MANGOWC_REPOSITORY), mango);
        }
    }
}

#[test]
fn borea_records_the_chosen_apps_and_extras_for_borea_remove() {
    // Arch: Vesktop from the AUR, Firefox from the repositories, the extras' packages.
    let job = scenario_apps(
        false,
        common::plan(Distribution::Arch, true).locale,
        false,
        Desktop::NiriNoctalia,
        true,
    );
    let added: serde_json::Value =
        serde_json::from_str(written(&job, "/var/lib/borea/apps.json")).unwrap();
    assert_eq!(added["vesktop"], serde_json::json!({"from": "aur", "package": "vesktop-bin"}));
    assert_eq!(added["firefox"]["from"], "native");
    assert_eq!(added["printing"]["packages"], serde_json::json!(["cups", "system-config-printer"]));
    // Tumbleweed: the same apps from its own repositories.
    let job = tumbleweed(Desktop::NiriNoctalia, false);
    let added: serde_json::Value =
        serde_json::from_str(written(&job, "/var/lib/borea/apps.json")).unwrap();
    assert_eq!(added["firefox"]["from"], "native");
    assert!(added["codecs"]["packages"].as_array().unwrap().contains(&"ffmpeg".into()));
}

fn cachyos_on(x86_level: u8) -> Job {
    let reference = scenario_target(false, common::plan(Distribution::Arch, true).locale, true);
    let mut hw = borea_catalog::hardware::samples().remove(0).1;
    hw.secure_boot = false;
    hw.x86_level = x86_level;
    let disk = Disk {
        id: reference.plan.disk.disk().clone(),
        readonly: false,
        in_use: false,
        live: false,
        removable: false,
        transport: "virtio".into(),
        parts: vec![],
    };
    compile(
        &reference.plan,
        &hw,
        &DiskFacts { uefi: true, disks: vec![disk] },
        &reference.bootstrap,
    )
    .unwrap()
}

#[test]
fn cachyos_uses_its_own_v3_repositories_ahead_of_arch_on_a_v3_cpu() {
    let job = cachyos_on(3);
    assert!(borea_backend::cachyos::optimised(&job.review.packages));
    let conf = written(&job, "/etc/pacman.conf");
    assert!(conf.contains("Architecture = x86_64 x86_64_v3\n"));
    let at = |repo: &str| conf.find(&format!("[{repo}]")).unwrap();
    assert!(at("cachyos-v3") < at("cachyos-core-v3"));
    assert!(at("cachyos-core-v3") < at("cachyos-extra-v3"));
    assert!(at("cachyos-extra-v3") < at("cachyos"));
    assert!(at("cachyos") < at("core"));
    // The mirror list and full upgrade happen before the first login, not after it.
    let queue = written(&job, "/var/lib/borea/firstboot.json");
    assert!(!queue.contains(borea_backend::cachyos::V3));
    let actions = job.stages.iter().flat_map(|s| &s.actions).collect::<Vec<_>>();
    let copy = actions.iter().position(|a| matches!(a, Action::CopyArchRepositories)).unwrap();
    let network = actions.iter().position(|a| matches!(a, Action::CopyNetwork)).unwrap();
    let upgrade = actions
        .iter()
        .position(|a| {
            matches!(a, Action::Run(c)
            if c.program == "/usr/bin/pacman" && c.args == ["-Syu", "--noconfirm"])
        })
        .unwrap();
    assert!(copy < network && network < upgrade, "the upgrade needs repositories and DNS");
    // The bootstrap installs CachyOS's v3 mirror list with the system.
    let pacstrap = actions
        .iter()
        .find_map(|a| match a {
            Action::Run(c) if c.program == "/usr/bin/pacstrap" => Some(c.args.clone()),
            _ => None,
        })
        .unwrap();
    assert!(pacstrap.iter().any(|p| p == borea_backend::cachyos::V3));
}

#[test]
fn cachyos_keeps_its_generic_repository_on_an_older_cpu() {
    let job = cachyos_on(2);
    assert!(!borea_backend::cachyos::optimised(&job.review.packages));
    let conf = written(&job, "/etc/pacman.conf");
    assert!(conf.contains("Architecture = auto\n"));
    assert!(!conf.contains("-v3]"));
    assert!(conf.find("[cachyos]").unwrap() < conf.find("[core]").unwrap());
}

#[test]
fn tumbleweed_apps_finish_after_login_and_zypper_waits_for_its_lock() {
    let job = tumbleweed(Desktop::NiriNoctalia, false);
    let queue: Vec<serde_json::Value> =
        serde_json::from_str(written(&job, "/var/lib/borea/firstboot.json")).unwrap();
    let zypper = queue.iter().find(|q| q["program"] == "/usr/bin/zypper").unwrap();
    let args = zypper["args"].as_array().unwrap();
    assert_eq!(args[..4], ["--non-interactive", "install", "--auto-agree-with-licenses", "--"]);
    for app in ["MozillaFirefox", "steam", "cups"] {
        assert!(args.contains(&app.into()), "{app} waits for the first login");
    }
    // Nothing the first login itself needs is deferred.
    for needed in ["noctalia", "greetd", "niri", "kernel-default", "zypper", "glibc-locale"] {
        assert!(!args.contains(&needed.into()), "{needed} installs before reboot");
    }
    assert!(written(&job, "/etc/profile.d/borea-zypper.sh").contains("ZYPP_LOCK_TIMEOUT=300"));
    assert!(written(&job, "/etc/sudoers.d/20-borea-zypper").contains("ZYPP_LOCK_TIMEOUT"));
}
