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
    if distro == Distro::Arch {
        for name in catalog::groups::ARCH_NOCTALIA_PACKAGES {
            add(base, name);
        }
        for id in ["gaming", "codecs", "flatpak", "printing"] {
            for name in catalog::groups::arch_extra(id).unwrap_or_default() {
                add(
                    if name.starts_with("lib32-") || *name == "steam" {
                        "arch.multilib"
                    } else {
                        base
                    },
                    name,
                );
            }
        }
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
                let drivers = catalog::plan_drivers(&hw, distro);
                let compat = if distro == Distro::Debian {
                    catalog::drivers::debian_gaming_compat(&drivers)
                } else {
                    Vec::new()
                };
                for package in drivers.packages.into_iter().chain(compat) {
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
type DebianMetadata = BTreeSet<(String, String, String)>;
fn parse_debian_metadata(text: &str) -> DebianMetadata {
    text.split("\n\n")
        .filter_map(|record| {
            let field = |name: &str| record.lines().find_map(|line| line.strip_prefix(name));
            let filename = field("Filename: ")?.strip_prefix("pool/")?;
            let filename = filename.strip_prefix("updates/").unwrap_or(filename);
            Some((
                field("Package: ")?.to_owned(),
                field("Architecture: ")?.to_owned(),
                filename.split('/').next()?.to_owned(),
            ))
        })
        .collect()
}
fn debian_matches(metadata: &DebianMetadata, entry: &Entry) -> bool {
    let (name, arch) = entry.name.split_once(':').unwrap_or((&entry.name, "amd64"));
    let Some(component) = entry.source.strip_prefix("debian.") else { return false };
    metadata.contains(&(name.into(), arch.into(), component.into()))
        || metadata.contains(&(name.into(), "all".into(), component.into()))
}
fn zypper_has_package_row(text: &str) -> bool {
    text.lines().any(|line| line.split('|').nth(2).is_some_and(|kind| kind.trim() == "package"))
}
fn query(distro: Distro, entry: &Entry, debian: Option<&DebianMetadata>) -> Result<(), Hint> {
    if !entry.source.contains('.') {
        return Ok(());
    }
    if let Some(metadata) = debian {
        return if debian_matches(metadata, entry) {
            Ok(())
        } else {
            Err(hint(
                "verify",
                format!(
                    "{}: {} is missing from its declared component/architecture",
                    entry.source, entry.name
                ),
            ))
        };
    }
    let (program, args) = match distro {
        Distro::Arch => ("/usr/bin/pacman", vec!["-Si", entry.name.as_str()]),
        Distro::Debian => ("/usr/bin/apt-cache", vec!["show", entry.name.as_str()]),
        Distro::Fedora if entry.name.starts_with('@') => (
            "/usr/bin/dnf",
            vec!["environment", "info", entry.name.trim_start_matches('@').trim_start_matches('^')],
        ),
        Distro::Fedora => (
            "/usr/bin/dnf",
            vec!["repoquery", "--available", "--queryformat=%{name}\\n", entry.name.as_str()],
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
    if distro == Distro::Arch && !output.status.success() {
        let group = Command::new("/usr/bin/pacman")
            .args(["-Sg", entry.name.as_str()])
            .output()
            .map_err(|e| hint("verify", e.to_string()))?;
        if group.status.success() && !group.stdout.is_empty() {
            return Ok(());
        }
    }
    if distro == Distro::OpenSuse && !output.status.success() {
        // Tumbleweed's versioned Python packages and QEMU packages expose
        // native capabilities; `info --type package` only checks exact names.
        let providers = Command::new("/usr/bin/zypper")
            .args([
                "--non-interactive",
                "search",
                "--details",
                "--match-exact",
                "--provides",
                &entry.name,
            ])
            .env("LC_ALL", "C")
            .output()
            .map_err(|e| hint("verify", e.to_string()))?;
        if providers.status.success()
            && zypper_has_package_row(&String::from_utf8_lossy(&providers.stdout))
        {
            return Ok(());
        }
    }
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
    if distro == Distro::Debian {
        let component = entry
            .source
            .strip_prefix("debian.")
            .ok_or_else(|| hint("verify", "Invalid Debian component."))?;
        if !String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.strip_prefix("Filename: "))
            .any(|filename| filename.split('/').any(|part| part == component))
        {
            return Err(hint(
                "verify",
                format!(
                    "{}: {} is not published in its declared Debian component",
                    entry.source, entry.name
                ),
            ));
        }
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
                "Use borea-verify arch|debian|fedora|opensuse [--check-native|--check-external|--check-flathub|--check-noctalia (Arch only)].",
            ));
        }
    };
    let entries = inventory(distro);
    let mode = args.get(1).map(String::as_str);
    if mode == Some("--check-noctalia") && distro == Distro::Arch {
        println!("Arch has the latest stable Noctalia v{}.", borea_backend::noctalia::resolve()?);
        return Ok(());
    }
    if mode.is_none() {
        println!(
            "{}",
            serde_json::to_string_pretty(&entries).map_err(|e| hint("verify", e.to_string()))?
        );
        return Ok(());
    }
    if !matches!(mode, Some("--check-native" | "--check-external" | "--check-flathub")) {
        return Err(hint("verify", "Unknown verification mode."));
    }
    // APT otherwise reparses its entire cache for every package. Query once, then
    // validate each requested name, architecture and component independently.
    let debian = if distro == Distro::Debian && mode == Some("--check-native") {
        let output = Command::new("/usr/bin/apt-cache")
            .arg("show")
            .args(entries.iter().filter(|e| e.source.starts_with("debian.")).map(|e| &e.name))
            .env("LC_ALL", "C")
            .output()
            .map_err(|e| hint("verify", e.to_string()))?;
        // A missing name makes apt-cache exit nonzero while retaining other
        // records. The per-entry checks below still report every missing name.
        Some(parse_debian_metadata(&String::from_utf8_lossy(&output.stdout)))
    } else {
        None
    };
    let mut failures = Vec::new();
    for entry in &entries {
        let external = !entry.source.contains('.');
        if mode == Some("--check-external") && !external
            || mode == Some("--check-flathub") && entry.source != "flathub"
        {
            continue;
        }
        let result = if external && mode != Some("--check-native") {
            borea_backend::external::verify_source(&entry.source, &entry.name)
        } else {
            query(distro, entry, debian.as_ref())
        };
        match result {
            Ok(()) if !external || mode != Some("--check-native") => {
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
        Err(hint("verify", format!("{} catalog entries failed validation.", failures.len())))
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
    fn zypper_provider_audit_requires_a_real_package_row() {
        assert!(!zypper_has_package_row(
            "S | Name | Type | Version | Arch | Repository\nNo matching items found.\n"
        ));
        assert!(zypper_has_package_row(
            "S | Name | Type | Version | Arch | Repository\n  | python313 | package | 3.13.9 | x86_64 | repo-oss\n"
        ));
        assert!(!zypper_has_package_row("  | desktop | pattern | 1 | x86_64 | repo-oss\n"));
    }
    #[test]
    fn debian_audit_requires_the_actual_component_and_architecture() {
        let metadata = parse_debian_metadata(
            "Package: lutris\nArchitecture: all\nFilename: pool/contrib/l/lutris/lutris.deb\n\nPackage: nvidia-driver-libs\nArchitecture: amd64\nFilename: pool/non-free/n/nvidia/nvidia.deb\n",
        );
        assert!(debian_matches(
            &metadata,
            &Entry { source: "debian.contrib".into(), name: "lutris".into() }
        ));
        assert!(!debian_matches(
            &metadata,
            &Entry { source: "debian.main".into(), name: "lutris".into() }
        ));
        assert!(!debian_matches(
            &metadata,
            &Entry { source: "debian.non-free".into(), name: "nvidia-driver-libs:i386".into() }
        ));
        assert!(!debian_matches(
            &metadata,
            &Entry { source: "debian.main".into(), name: "missing".into() }
        ));
        let security = parse_debian_metadata(
            "Package: firefox-esr\nArchitecture: amd64\nFilename: pool/updates/main/f/firefox-esr/firefox.deb\n",
        );
        assert!(debian_matches(
            &security,
            &Entry { source: "debian.main".into(), name: "firefox-esr".into() }
        ));
    }
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
