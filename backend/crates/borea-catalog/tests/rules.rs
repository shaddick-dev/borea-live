#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
use borea_catalog::*;

fn from(r: Resolution) -> Option<(Source, &'static str)> {
    match r {
        Resolution::From { source, package } => Some((source, package)),
        _ => None,
    }
}

#[test]
fn each_distro_only_offers_its_own_repos() {
    for d in Distro::ALL {
        let s = Sources::defaults(d);
        for sw in s.switches() {
            if let Source::Repo(id) = sw {
                assert_eq!(
                    borea_catalog::source::repo(id).unwrap().distro,
                    d,
                    "{id} leaked into {d:?}"
                );
            }
        }
        assert_eq!(s.switches().contains(&Source::Aur), d == Distro::Arch);
        assert!(s.switches().contains(&Source::Flathub));
    }
}

#[test]
fn foreign_repos_and_aur_off_arch_are_refused() {
    let mut s = Sources::defaults(Distro::Debian);
    assert!(matches!(
        s.set(Source::Repo("arch.multilib"), true),
        Err(SourceError::NotThisDistro(_))
    ));
    assert_eq!(s.set(Source::Aur, true), Err(SourceError::AurNeedsArch));
    assert_eq!(
        s.set(Source::Repo("debian.main"), false),
        Err(SourceError::BaseIsFixed("debian.main"))
    );
}

#[test]
fn steam_comes_from_each_distros_own_repo() {
    let steam = app("steam").unwrap();
    let want = [
        (Distro::Arch, "arch.multilib"),
        (Distro::Debian, "debian.non-free"),
        (Distro::OpenSuse, "opensuse.non-oss"),
    ];
    for (d, repo) in want {
        let (src, _) = from(resolve(steam, &Sources::defaults(d), Prefer::Native)).unwrap();
        assert_eq!(src, Source::Repo(repo), "{d:?}");
    }
    // RPM Fusion nonfree is off by default on Fedora, so Steam falls back to Flathub.
    let (src, id) =
        from(resolve(steam, &Sources::defaults(Distro::Fedora), Prefer::Native)).unwrap();
    assert_eq!((src, id), (Source::Flathub, "com.valvesoftware.Steam"));
}

#[test]
fn prefer_flatpak_flips_the_order() {
    let s = Sources::defaults(Distro::Arch);
    let gimp = app("gimp").unwrap();
    assert_eq!(from(resolve(gimp, &s, Prefer::Native)).unwrap().0, Source::Repo("arch.core-extra"));
    assert_eq!(from(resolve(gimp, &s, Prefer::Flatpak)).unwrap().0, Source::Flathub);
    // No Flathub build: still installs natively.
    let neovim = app("neovim").unwrap();
    assert_eq!(
        from(resolve(neovim, &s, Prefer::Flatpak)).unwrap().0,
        Source::Repo("arch.core-extra")
    );
}

#[test]
fn ticking_an_app_switches_on_the_source_it_needs() {
    let mut s = Sources::defaults(Distro::Arch);
    s.set(Source::Flathub, false).unwrap();
    let mut sel = Selection::default();
    // VSCodium has no Arch repo package: with Flathub off it needs the AUR.
    assert_eq!(sel.toggle("vscodium", &mut s, Prefer::Native), Pick::AddedAndEnabled(Source::Aur));
    assert!(s.is_on(Source::Aur));
    assert_eq!(sel.toggle("vscodium", &mut s, Prefer::Native), Pick::Removed);
}

#[test]
fn unavailable_apps_cannot_be_ticked() {
    let mut s = Sources::defaults(Distro::Debian);
    let mut sel = Selection::default();
    // Media codecs exist natively on Debian; Timeshift has no Flathub build and no openSUSE package.
    let mut suse = Sources::defaults(Distro::OpenSuse);
    assert_eq!(sel.toggle("timeshift", &mut suse, Prefer::Native), Pick::Unavailable);
    assert_eq!(sel.toggle("codecs", &mut s, Prefer::Native), Pick::Added);
}

#[test]
fn plan_groups_by_source_and_flags_stranded_apps() {
    let mut s = Sources::defaults(Distro::Debian);
    let sel = Selection::with(&["firefox", "discord", "steam", "gimp"]);
    let p = plan(&sel, &s, Prefer::Native);
    assert_eq!(p.batches[&Source::Repo("debian.main")], vec!["firefox-esr", "gimp"]);
    assert_eq!(p.batches[&Source::Repo("debian.non-free")], vec!["steam-installer"]);
    assert_eq!(p.batches[&Source::Flathub], vec!["com.discordapp.Discord"]);
    assert_eq!(p.count(), 4);
    // Switch Flathub off after picking: Discord is stranded rather than silently dropped.
    s.set(Source::Flathub, false).unwrap();
    let p = plan(&sel, &s, Prefer::Native);
    assert_eq!(p.stranded, vec!["discord"]);
}

#[test]
fn presets_only_name_real_apps() {
    for (name, ids, addons) in PRESETS {
        for id in *ids {
            assert!(app(id).is_some(), "preset {name} names unknown app {id}");
        }
        for a in *addons {
            assert!(
                group(a).is_some_and(|g| g.kind == Kind::AddOn),
                "preset {name} names unknown add-on {a}"
            );
        }
    }
}

#[test]
fn essentials_install_out_of_the_box_on_every_distro() {
    let ids = PRESETS.iter().find(|(n, _, _)| *n == "Essentials").unwrap().1;
    for d in Distro::ALL {
        let p = plan(&Selection::with(ids), &Sources::defaults(d), Prefer::Native);
        assert!(p.stranded.is_empty(), "{d:?} strands {:?}", p.stranded);
        assert_eq!(p.count(), ids.len(), "{d:?}");
    }
}

#[test]
fn new_browsers_resolve_through_allowed_sources_only() {
    let helium = app("helium").unwrap();
    let mut arch = Sources::defaults(Distro::Arch);
    // Mullvad comes from Flathub; Helium from its AppImage until the AUR is switched on.
    let mut sel = Selection::default();
    assert_eq!(sel.toggle("mullvad", &mut arch, Prefer::Native), Pick::Added);
    assert_eq!(sel.toggle("helium", &mut arch, Prefer::Native), Pick::Added);
    assert_eq!(from(resolve(helium, &arch, Prefer::Native)).unwrap().0, Source::AppImage);
    arch.set(Source::Aur, true).unwrap();
    assert_eq!(
        from(resolve(helium, &arch, Prefer::Native)),
        Some((Source::Aur, "helium-browser-bin"))
    );
    // No vendor repos, but the AppImage runs anywhere.
    for d in [Distro::Debian, Distro::Fedora, Distro::OpenSuse] {
        assert_eq!(
            from(resolve(helium, &Sources::defaults(d), Prefer::Native)),
            Some((Source::AppImage, "imputnet/helium-linux")),
            "{d:?}"
        );
    }
    assert!(
        !listed(app("brave").unwrap())
            && !listed(app("tor").unwrap())
            && listed(app("helium").unwrap())
    );
}

#[test]
fn every_group_package_exists_for_every_distro() {
    for g in GROUPS {
        for k in g.offers(true) {
            if g.apps.contains(&k) {
                assert!(app(k).is_some(), "{} offers unknown app {k}", g.id);
                continue;
            }
            for d in Distro::ALL {
                assert!(package(k, d).is_some(), "{} needs '{k}' mapped for {d:?}", g.id);
            }
        }
    }
}

#[test]
fn servers_have_no_desktop_and_skip_desktop_apps() {
    for env in ["minimal", "server"] {
        let p = plan_groups(env, &["monitor"], "plasma", Distro::Fedora).unwrap();
        assert_eq!(p.desktop, None);
        assert!(!p.apps_allowed);
    }
    let ws = plan_groups("workstation", &[], "gnome", Distro::Debian).unwrap();
    assert_eq!(ws.desktop, Some(("task-gnome-desktop", "gdm3")));
    assert!(ws.apps_allowed);
}

#[test]
fn add_ons_merge_without_duplicates_and_gui_extras_need_a_desktop() {
    // Server already has cockpit; ticking Virtualization's cockpit-machines adds only that.
    let mut picks = Picks::default();
    picks.set("virt", "cockpit-machines", true).unwrap();
    let p = plan_groups_with("server", &["virt", "containers"], &picks, "plasma", Distro::Debian)
        .unwrap();
    assert_eq!(p.packages.iter().filter(|x| **x == "cockpit").count(), 1);
    assert!(p.packages.contains(&"cockpit-machines") && !p.packages.contains(&"cockpit-podman"));
    assert!(!p.packages.contains(&"virt-manager"), "no virt-manager without a desktop");
    let g = plan_groups("server-gui", &["virt"], "plasma", Distro::Debian).unwrap();
    assert!(g.packages.contains(&"virt-manager"));
}

#[test]
fn only_environments_and_add_ons_in_their_slots() {
    assert_eq!(
        plan_groups("virt", &[], "plasma", Distro::Arch),
        Err(GroupError::NotAnEnvironment("virt".into()))
    );
    assert_eq!(
        plan_groups("server", &["workstation"], "plasma", Distro::Arch),
        Err(GroupError::NotAnAddOn("workstation".into()))
    );
}

#[test]
fn appimage_is_a_last_resort_on_every_distro() {
    for d in Distro::ALL {
        let mut s = Sources::defaults(d);
        assert_eq!(s.switches().last(), Some(&Source::AppImage));
        // Apps with a packaged source never fall back to an AppImage.
        for a in APPS.iter().filter(|a| a.appimage.is_some()) {
            for p in [Prefer::Native, Prefer::Flatpak] {
                assert_eq!(candidates(a, d, p).last().map(|c| c.0), Some(Source::AppImage));
            }
        }
        s.set(Source::AppImage, false).unwrap();
        let mut sel = Selection::default();
        assert_eq!(
            sel.toggle("helium", &mut s, Prefer::Native),
            Pick::AddedAndEnabled(if d == Distro::Arch { Source::Aur } else { Source::AppImage })
        );
    }
}

#[test]
fn every_package_can_be_ticked_except_what_boots_the_system() {
    let mut picks = Picks::default();
    picks.set("server", "cockpit", false).unwrap();
    picks.set("web", "certbot", false).unwrap();
    picks.set("database", "mariadb", true).unwrap();
    assert_eq!(picks.set("server", "kernel", false), Err(GroupError::Required("kernel")));
    assert_eq!(picks.set("web", "samba", true), Err(GroupError::NotInGroup("samba".into())));
    let p =
        plan_groups_with("server", &["web", "database"], &picks, "plasma", Distro::Fedora).unwrap();
    assert!(!p.packages.contains(&"cockpit") && !p.packages.contains(&"certbot"));
    assert!(
        p.packages.contains(&"nginx")
            && p.packages.contains(&"mariadb-server")
            && p.packages.contains(&"postgresql-server")
    );
    assert!(p.packages.contains(&"kernel"));
}

#[test]
fn optional_extras_start_unticked() {
    let p = plan_groups("workstation", &["dev", "network"], "plasma", Distro::Arch).unwrap();
    assert!(p.packages.contains(&"base-devel") && p.packages.contains(&"git"));
    assert!(
        !p.packages.contains(&"rustup")
            && !p.packages.contains(&"wireshark-cli")
            && !p.packages.contains(&"openssh")
    );
}

#[test]
fn cockpit_is_a_server_thing() {
    assert!(group("server").unwrap().items.contains(&"cockpit"));
    assert!(group("server-gui").unwrap().items.contains(&"cockpit"));
    assert!(!group("workstation").unwrap().offers(true).any(|k| k == "cockpit"));
    for g in GROUPS.iter().filter(|g| g.kind == Kind::AddOn) {
        assert!(SECTIONS.contains(&g.section), "{} has no section", g.id);
    }
}

#[test]
fn apps_and_add_ons_never_overlap() {
    // A command-line tool is an add-on package; a desktop app is a catalogue app. Never both.
    for d in Distro::ALL {
        let group_pkgs: Vec<&str> =
            GROUPS.iter().flat_map(|g| g.offers(true)).filter_map(|k| package(k, d)).collect();
        for a in APPS {
            for n in a.native.iter().filter(|n| n.distro == d) {
                assert!(
                    !group_pkgs.contains(&n.pkg),
                    "{} ({}) is both an app and an add-on package on {d:?}",
                    a.id,
                    n.pkg
                );
            }
        }
    }
}

#[test]
fn file_sharing_brings_localsend_on_desktops() {
    let ws = plan_groups("workstation", &["files"], "plasma", Distro::Fedora).unwrap();
    assert_eq!(ws.apps, vec!["localsend"]);
    assert!(ws.packages.contains(&"samba") && !ws.packages.contains(&"localsend"));
    for rule in ["samba", "53317/tcp", "53317/udp"] {
        assert!(ws.firewall.contains(&rule), "{rule} not opened");
    }
    // No desktop, no LocalSend (and nothing to open for it).
    let srv = plan_groups("server", &["files"], "plasma", Distro::Fedora).unwrap();
    assert!(
        srv.apps.is_empty()
            && !srv.firewall.contains(&"53317/tcp")
            && srv.firewall.contains(&"cockpit")
    );
    // Untickable like any package.
    let mut picks = Picks::default();
    picks.set("files", "localsend", false).unwrap();
    assert!(
        plan_groups_with("workstation", &["files"], &picks, "plasma", Distro::Arch)
            .unwrap()
            .apps
            .is_empty()
    );
    // Installs out of the box everywhere (Flathub), and isn't listed under Applications.
    for d in Distro::ALL {
        assert!(
            matches!(
                resolve(app("localsend").unwrap(), &Sources::defaults(d), Prefer::Native),
                Resolution::From { .. }
            ),
            "{d:?}"
        );
    }
    assert_eq!(addon_for_app("localsend").map(|g| g.id), Some("files"));
}

#[test]
fn minimal_has_no_firewall_to_open() {
    let p = plan_groups("minimal", &["web"], "plasma", Distro::Debian).unwrap();
    assert!(p.firewall.is_empty());
}

#[test]
fn verified_missing_debian_cosmic_is_blocked_before_install() {
    assert!(borea_catalog::groups::desktop_unavailable(Distro::Debian, "cosmic").is_some());
    assert!(borea_catalog::plan_groups("workstation", &[], "cosmic", Distro::Debian).is_err());
}
