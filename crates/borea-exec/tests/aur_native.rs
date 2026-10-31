#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#[path = "../../../backend/tests/common.rs"]
mod common;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

/// Explicit fixture only: never attach host devices or run package operations on
/// the developer's machine. CI runs this inside its disposable Arch container.
#[test]
#[ignore = "requires disposable Arch container and the explicit AUR fixture marker"]
fn pinned_vesktop_binary_installs_unprivileged_and_resumes_after_checkpoint() {
    assert!(nix::unistd::Uid::effective().is_root());
    assert!(Path::new("/.dockerenv").is_file());
    assert_eq!(fs::read_to_string("/fixtures/.borea-aur-fixture").unwrap(), "BOREA_TEST_AUR\n");
    assert!(fs::read_to_string("/etc/os-release").unwrap().lines().any(|l| l == "ID=arch"));
    assert!(!Path::new("/var/lib/borea/aur.json").exists());
    let mut plan = common::plan(borea_core::Distribution::Arch, true);
    plan.desktop = Some(borea_core::Desktop::HyprlandNoctalia);
    plan.env = "composed".into();
    plan.apps = ["vesktop".to_owned()].into();
    plan.prefer = borea_core::SourcePreference::Aur;
    let hw = borea_catalog::hardware::samples().remove(0).1;
    let review = borea_core::review(&plan, &hw).unwrap();
    assert!(review.packages.iter().any(|p| p.source == "aur" && p.name == "vesktop-bin"));
    let external = borea_backend::external::resolve(&review).unwrap();
    assert_eq!(external.aur.len(), 1);
    let directory = Path::new("/var/lib/borea/aur");
    fs::create_dir_all(directory).unwrap();
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
    for recipe in &external.aur {
        borea_exec::download(
            &recipe.url,
            &directory.join(format!("{}.tar.gz", recipe.base)),
            Some(&recipe.sha256),
        )
        .unwrap();
        println!("AUR_PINNED {} {}", recipe.base, recipe.sha256);
    }
    fs::write("/var/lib/borea/aur.json", serde_json::to_vec(&external.aur).unwrap()).unwrap();
    fs::set_permissions("/var/lib/borea/aur.json", fs::Permissions::from_mode(0o600)).unwrap();
    let native = fs::read_to_string("/etc/pacman.conf").unwrap();
    let strict = native
        .lines()
        .filter(|line| !line.trim().starts_with("LocalFileSigLevel"))
        .map(|line| {
            if line.trim() == "[options]" {
                "[options]\nLocalFileSigLevel = Required"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert!(
        strict.contains("LocalFileSigLevel = Required"),
        "fixture must exercise the strict target policy"
    );
    fs::write("/etc/pacman.conf", &strict).unwrap();
    borea_exec::postinstall::aur_apps().unwrap();
    assert_eq!(
        fs::read_to_string("/etc/pacman.conf").unwrap(),
        strict,
        "remote and system policy remain unchanged"
    );
    assert!(
        Command::new("/usr/bin/pacman").args(["-Q", "vesktop-bin"]).status().unwrap().success()
    );
    assert!(Path::new("/usr/bin/vesktop").is_file());
    let progress = fs::read("/var/lib/borea/aur-progress.json").unwrap();
    assert!(String::from_utf8_lossy(&progress).contains(&external.aur[0].sha256));
    // An interrupted outer queue may call us again after the package checkpoint.
    // With the archive removed, succeeding proves the completed recipe is skipped.
    for recipe in &external.aur {
        fs::remove_file(directory.join(format!("{}.tar.gz", recipe.base))).unwrap();
    }
    borea_exec::postinstall::aur_apps().unwrap();
    assert_eq!(fs::read("/var/lib/borea/aur-progress.json").unwrap(), progress);
    assert!(
        !fs::read_to_string("/etc/passwd").unwrap().lines().any(|l| l.starts_with("borea-build:"))
    );
    println!("BOREA_PINNED_AUR_INSTALL_AND_RETRY_PASSED");
}
