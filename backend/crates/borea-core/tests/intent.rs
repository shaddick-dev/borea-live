#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "../../../tests/common.rs"]
mod common;
use borea_core::*;
#[test]
fn plan_round_trip_and_no_implicit_distro() {
    let p = common::plan(Distribution::Arch, true);
    let text = toml::to_string(&p).unwrap();
    assert_eq!(p, toml::from_str::<Plan>(&text).unwrap());
    let v = serde_json::to_value(&p).unwrap();
    let mut missing = v.clone();
    missing.as_object_mut().unwrap().remove("distro");
    assert!(serde_json::from_value::<Plan>(missing).is_err());
    let mut injection = v;
    injection["passphrase"] = serde_json::json!("do-not-persist");
    assert!(serde_json::from_value::<Plan>(injection).is_err());
}
#[test]
fn identity_and_argument_injection() {
    for name in ["root", "daemon", "nobody", "-bad", "a:b", "Auser", "a\nroot", "1name", ""] {
        assert!(!valid_username(name), "{name:?}");
    }
    for name in ["user", "first-last", "_operator", "user123"] {
        assert!(valid_username(name));
    }
    for host in ["-host", "host-", "evil\nhost", "host.name", ""] {
        assert!(!valid_hostname(host));
    }
    let mut p = common::plan(Distribution::Debian, true);
    p.locale.keyboard.layout = "-option".into();
    assert!(p.validate_intent().is_err());
}
#[test]
fn navigation_skips_desktop_both_directions() {
    let p = common::plan(Distribution::Debian, false);
    assert_eq!(Step::Distro.next(&p), Step::System);
    assert_eq!(Step::System.prev(&p), Step::Distro);
    let p = common::plan(Distribution::Debian, true);
    assert_eq!(Step::Distro.next(&p), Step::Desktop);
    assert_eq!(Step::System.prev(&p), Step::Desktop);
}
#[test]
fn protected_packages_and_profile_constraints() {
    let mut p = common::plan(Distribution::Arch, false);
    p.picks.push(PackagePick { group: "minimal".into(), key: "kernel".into(), on: false });
    assert!(p.validate_intent().is_err());
    p.picks.clear();
    p.apps.insert("firefox".into());
    assert!(p.validate_intent().is_err());
    p.apps.clear();
    p.desktop = Some(Desktop::Gnome);
    assert!(p.validate_intent().is_err());
}
#[test]
fn drivers_lock_sources_and_stranded_apps_fail() {
    for (_, hw) in borea_catalog::hardware::samples() {
        for d in Distribution::ALL {
            let mut p = common::plan(d, true);
            for r in borea_catalog::repos_for(d).filter(|r| !r.base) {
                p.sources.insert(r.id.into(), false);
            }
            p.sources.insert("flathub".into(), false);
            p.sources.insert("appimage".into(), false);
            let review = review(&p, &hw).unwrap();
            assert!(!review.packages.is_empty());
            let unique = review
                .packages
                .iter()
                .map(|p| (&p.source, &p.name))
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(unique.len(), review.packages.len());
        }
    }
    let (_, hw) = borea_catalog::hardware::samples().remove(0);
    let mut p = common::plan(Distribution::Debian, true);
    p.apps.insert("brave".into());
    p.sources.insert("flathub".into(), false);
    assert!(review(&p, &hw).is_err());
}
#[test]
fn secrets_are_redacted_and_separate_admin_required() {
    let mut id = common::plan(Distribution::Arch, true).user;
    id.same_passphrase = false;
    let secrets =
        Secrets { wifi: None, passphrase: Secret::new("correct horse".into()), admin: None };
    assert!(secrets.validate(&id).is_err());
    assert!(!format!("{secrets:?}").contains("correct horse"));
}
#[test]
fn passphrase_limits_match_interactive_disk_unlock_bytes() {
    let identity = common::plan(Distribution::Arch, false).user;
    for value in ["x".repeat(512), "💨".repeat(128)] {
        assert!(
            Secrets { wifi: None, passphrase: Secret::new(value), admin: None }
                .validate(&identity)
                .is_ok()
        );
    }
    for value in ["x".repeat(513), "💨".repeat(129)] {
        assert!(
            Secrets { wifi: None, passphrase: Secret::new(value), admin: None }
                .validate(&identity)
                .is_err()
        );
    }
}
#[test]
fn native_debian_gaming_uses_contrib_and_matching_32_bit_driver_libraries() {
    let mut plan = common::plan(Distribution::Debian, true);
    plan.apps.insert("steam".into());
    plan.sources.insert("debian.contrib".into(), true);
    plan.sources.insert("flathub".into(), false);
    for (_, hw) in borea_catalog::hardware::samples() {
        let result = review(&plan, &hw).unwrap();
        assert!(
            result
                .packages
                .iter()
                .any(|p| p.name == "steam-installer" && p.source == "debian.contrib")
        );
        if result.packages.iter().any(|p| p.name == "nvidia-driver") {
            assert!(
                result
                    .packages
                    .iter()
                    .any(|p| p.name == "nvidia-driver-libs:i386" && p.source == "debian.non-free")
            );
        }
    }
}

#[test]
fn arch_signed_boot_requires_an_explicit_choice_and_setup_mode() {
    let mut plan = common::plan(Distribution::Arch, false);
    let (_, mut hw) = borea_catalog::hardware::samples().remove(0);
    hw.secure_boot = false;
    hw.setup_mode = Some(true);
    assert!(!plan.target_secure_boot(&hw));
    plan.secure_boot = Some(true);
    assert!(plan.target_secure_boot(&hw));
    assert!(plan.validate_firmware(&hw).is_ok());
    hw.setup_mode = Some(false);
    assert!(plan.validate_firmware(&hw).is_err());
    plan.secure_boot = Some(false);
    hw.secure_boot = true;
    assert!(plan.validate_firmware(&hw).is_err());
}
