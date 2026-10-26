#![forbid(unsafe_code)]
//! Read-only release gate, run inside the selected distribution's official container.
use borea_catalog::{self as catalog, Distro};
use borea_core::{Hint, hint};
use serde::Serialize;
use std::{collections::BTreeSet, process::Command};
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct Entry {
    source: String,
    name: String,
}
fn inventory(distro: Distro) -> BTreeSet<Entry> {
    let mut entries = BTreeSet::new();
    let base = catalog::repos_for(distro).find(|r| r.base).map_or("", |r| r.id);
    let mut add = |source: &str, name: &str| {
        entries.insert(Entry { source: source.into(), name: name.into() });
    };
    for (key, _) in catalog::PKGS {
        if let Some(name) = catalog::package(key, distro) {
            add(
                if distro == Distro::Debian && *key == "firmware" {
                    "debian.non-free-firmware"
                } else {
                    base
                },
                name,
            );
        }
    }
    let index = match distro {
        Distro::Arch => 0,
        Distro::Debian => 1,
        Distro::Fedora => 2,
        Distro::OpenSuse => 3,
    };
    for (id, choices) in catalog::DESKTOPS {
        if catalog::groups::desktop_unavailable(distro, id).is_none() {
            add(base, choices[index].0);
            add(base, choices[index].1);
        }
    }
    for app in catalog::APPS {
        for native in app.native.iter().filter(|n| n.distro == distro) {
            add(native.repo, native.pkg);
        }
        if distro == Distro::Arch {
            if let Some(aur) = app.aur {
                add("aur", aur);
            }
        }
        if let Some(id) = app.flatpak {
            add("flathub", id);
        }
        if let Some(repo) = app.appimage {
            add("appimage", repo);
        }
    }
    for name in borea_backend::boot_packages(distro) {
        add(base, name);
    }
    let families = [
        catalog::Family::Nvidia(catalog::NvGen::Older),
        catalog::Family::Nvidia(catalog::NvGen::Kepler),
        catalog::Family::Nvidia(catalog::NvGen::Maxwell),
        catalog::Family::Nvidia(catalog::NvGen::Pascal),
        catalog::Family::Nvidia(catalog::NvGen::Volta),
        catalog::Family::Nvidia(catalog::NvGen::Turing),
        catalog::Family::Nvidia(catalog::NvGen::Ampere),
        catalog::Family::Nvidia(catalog::NvGen::Hopper),
        catalog::Family::Nvidia(catalog::NvGen::Ada),
        catalog::Family::Nvidia(catalog::NvGen::Blackwell),
        catalog::Family::Amd(catalog::hardware::AmdGen::TeraScale),
        catalog::Family::Amd(catalog::hardware::AmdGen::Gcn1),
        catalog::Family::Amd(catalog::hardware::AmdGen::Gcn2),
        catalog::Family::Amd(catalog::hardware::AmdGen::Modern),
        catalog::Family::Intel(catalog::hardware::IntelGen::Legacy),
        catalog::Family::Intel(catalog::hardware::IntelGen::Modern),
        catalog::Family::Intel(catalog::hardware::IntelGen::Arc),
        catalog::Family::Other,
    ];
    for (_, mut hw) in catalog::hardware::samples() {
        hw.broadcom_wl = true;
        hw.sof_audio = true;
        hw.fingerprint = Some("test-supported-vendor".into());
        for cpu in [catalog::Cpu::Intel, catalog::Cpu::Amd] {
            hw.cpu = cpu;
            for family in families {
                for gpu in &mut hw.gpus {
                    gpu.family = family;
                }
                for package in catalog::plan_drivers(&hw, distro).packages {
                    add(package.source.key(), package.name);
                }
            }
        }
    }
    for name in ["flatpak", "mokutil"] {
        add(base, name);
    }
    if distro == Distro::Arch {
        for name in ["base-devel", "git", "sbctl", "sbsigntools", "systemd-ukify"] {
            add(base, name);
        }
    }
    if distro == Distro::OpenSuse {
        add(base, "glibc-locale");
    }
    entries
}
fn query(distro: Distro, entry: &Entry) -> Result<(), Hint> {
    if !entry.source.contains('.') {
        return Ok(());
    }
    let (program, args) = match distro {
        Distro::Arch => ("/usr/bin/pacman", vec!["-Sp", "--noconfirm", entry.name.as_str()]),
        Distro::Debian => ("/usr/bin/apt-cache", vec!["show", entry.name.as_str()]),
        Distro::Fedora if entry.name.starts_with('@') => (
            "/usr/bin/dnf",
            vec!["environment", "info", entry.name.trim_start_matches('@').trim_start_matches('^')],
        ),
        Distro::Fedora => (
            "/usr/bin/dnf",
            vec!["repoquery", "--available", "--queryformat=%{name}", entry.name.as_str()],
        ),
        Distro::OpenSuse => (
            "/usr/bin/zypper",
            vec!["--non-interactive", "info", "--type", "package", entry.name.as_str()],
        ),
    };
    let output = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| hint("verify", e.to_string()))?;
    if !output.status.success()
        || output.stdout.is_empty()
        || distro == Distro::Fedora
            && !entry.name.starts_with('@')
            && !String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.trim() == entry.name)
    {
        return Err(hint(
            "verify",
            format!("{}: {} is missing or its repository is unavailable", entry.source, entry.name),
        ));
    }
    Ok(())
}
fn run() -> Result<(), Hint> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let distro = match args.first().map(String::as_str) {
        Some("arch") => Distro::Arch,
        Some("debian") => Distro::Debian,
        Some("fedora") => Distro::Fedora,
        Some("opensuse") => Distro::OpenSuse,
        _ => {
            return Err(hint(
                "verify",
                "Use borea-verify arch|debian|fedora|opensuse [--check-native].",
            ));
        }
    };
    let entries = inventory(distro);
    if args.get(1).map(String::as_str) != Some("--check-native") {
        println!(
            "{}",
            serde_json::to_string_pretty(&entries).map_err(|e| hint("verify", e.to_string()))?
        );
        return Ok(());
    }
    let mut failures = Vec::new();
    for entry in &entries {
        match query(distro, entry) {
            Ok(()) if entry.source.contains('.') => {
                println!("PASS {} {}", entry.source, entry.name)
            }
            Ok(()) => println!("EXTERNAL {} {}", entry.source, entry.name),
            Err(error) => {
                eprintln!("FAIL {}", error.message);
                failures.push(error.message);
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(hint("verify", format!("{} native catalog entries failed validation.", failures.len())))
    }
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_covers_sources_and_distro_boundaries() {
        for distro in Distro::ALL {
            let entries = inventory(distro);
            assert!(entries.len() > 100);
            for entry in entries.iter().filter(|p| p.source.contains('.')) {
                assert!(catalog::repos_for(distro).any(|r| r.id == entry.source));
            }
            if distro != Distro::Arch {
                assert!(!entries.iter().any(|p| p.source == catalog::Source::Aur.key()));
            }
        }
    }
}
