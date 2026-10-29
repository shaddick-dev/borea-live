//! `borea extras`, `borea install`, `borea remove`: the extras Borea Live offers
//! (Gaming, Media codecs, Flatpak, Printing), on the installed system, with the
//! distribution's own package manager. Borea keeps no package database of its
//! own: it records only which packages it added for which extra, so `remove`
//! never takes away anything you installed yourself.
//!
//! Arch-family systems (Arch Linux, CachyOS, Omarchy) first; others say so.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use borea_live_catalog::Catalog;
use borea_live_core::DistroId;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// What Borea added, per extra: `extra package package …`, one line each.
const RECORD: &str = "/var/lib/borea/extras";

/// The catalog distribution this system is, from os-release (and Omarchy's own
/// files, since Omarchy reports itself as Arch).
pub fn detect(os_release: &str, omarchy: bool) -> Option<&'static str> {
    let field = |key: &str| {
        os_release.lines().find_map(|l| l.strip_prefix(key)).map(|v| v.trim().trim_matches('"'))
    };
    let id = field("ID=").unwrap_or_default();
    let like = field("ID_LIKE=").unwrap_or_default();
    Some(match id {
        "arch" if omarchy => "omarchy",
        "arch" => "arch",
        "cachyos" => "cachyos",
        "fedora" => "fedora",
        "nobara" => "nobara",
        "bazzite" => "bazzite",
        "debian" => "debian",
        "pikaos" => "pikaos",
        "opensuse-tumbleweed" => "tumbleweed",
        _ if like.split_whitespace().any(|l| l == "arch") => "arch",
        _ => return None,
    })
}

/// Systems `borea install` can change today: those using pacman.
pub fn pacman(distro: &str) -> bool {
    matches!(distro, "arch" | "cachyos" | "omarchy")
}

/// What installing some extras means on one distribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Extra id → its packages, in the order asked.
    pub extras: Vec<(String, Vec<String>)>,
    pub multilib: bool,
}

impl Plan {
    pub fn packages(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        self.extras
            .iter()
            .flat_map(|(_, p)| p.iter())
            .filter(|p| seen.insert(p.as_str()))
            .cloned()
            .collect()
    }
}

pub fn plan(catalog: &Catalog, distro: &str, ids: &[&str]) -> Result<Plan> {
    let offered: Vec<_> = catalog.extras.on(&DistroId(distro.to_owned())).collect();
    let mut plan = Plan { extras: Vec::new(), multilib: false };
    for id in ids {
        let Some(o) = offered.iter().find(|o| o.extra.id == *id) else {
            let known: Vec<&str> = offered.iter().map(|o| o.extra.id.as_str()).collect();
            return Err(match catalog.extras.get(id) {
                Some(e) => format!("{} isn't offered on {distro}.", e.name).into(),
                None => format!("No extra called \"{id}\". Try: {}", known.join(", ")).into(),
            });
        };
        if plan.extras.iter().any(|(e, _)| e == id) {
            continue;
        }
        if o.provision.built_in {
            continue;
        }
        plan.multilib |= o.provision.needs.as_deref().is_some_and(|n| n.contains("multilib"));
        plan.extras.push((o.extra.id.clone(), o.provision.packages.clone()));
    }
    Ok(plan)
}

/// pacman.conf with the multilib repository on, or `None` when it already is.
pub fn with_multilib(conf: &str) -> Option<String> {
    if conf.lines().any(|l| l.trim() == "[multilib]") {
        return None;
    }
    let lines: Vec<&str> = conf.lines().collect();
    if let Some(at) = lines.iter().position(|l| l.trim() == "#[multilib]") {
        let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
        out[at] = "[multilib]".into();
        if let Some(next) = out.get_mut(at + 1)
            && next.trim_start().starts_with("#Include")
        {
            *next = next.trim_start().trim_start_matches('#').to_owned();
        }
        return Some(out.join("\n") + "\n");
    }
    Some(format!("{}\n[multilib]\nInclude = /etc/pacman.d/mirrorlist\n", conf.trim_end()))
}

/// 32-bit Vulkan for a graphics vendor (PCI id), so pacman picks no stranger.
pub fn lib32_vulkan(vendor: &str) -> Option<&'static str> {
    match vendor.trim() {
        "0x8086" => Some("lib32-vulkan-intel"),
        "0x1002" => Some("lib32-vulkan-radeon"),
        "0x10de" => Some("lib32-nvidia-utils"),
        _ => None,
    }
}

fn graphics_vendors() -> Vec<String> {
    let mut vendors = Vec::new();
    for dev in fs::read_dir("/sys/bus/pci/devices").into_iter().flatten().flatten() {
        let read = |f: &str| fs::read_to_string(dev.path().join(f)).unwrap_or_default();
        if read("class").starts_with("0x03") {
            vendors.push(read("vendor").trim().to_owned());
        }
    }
    vendors
}

pub fn parse_record(text: &str) -> BTreeMap<String, Vec<String>> {
    text.lines()
        .filter_map(|l| {
            let mut words = l.split_whitespace();
            Some((words.next()?.to_owned(), words.map(str::to_owned).collect()))
        })
        .collect()
}

pub fn write_record(record: &BTreeMap<String, Vec<String>>) -> String {
    record.iter().map(|(e, p)| format!("{e} {}\n", p.join(" "))).collect::<Vec<_>>().concat()
}

fn record() -> BTreeMap<String, Vec<String>> {
    parse_record(&fs::read_to_string(RECORD).unwrap_or_default())
}

fn save(record: &BTreeMap<String, Vec<String>>) -> Result {
    if let Some(dir) = Path::new(RECORD).parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(RECORD, write_record(record))?;
    Ok(())
}

fn system() -> Result<&'static str> {
    let os = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let omarchy = Path::new("/usr/share/omarchy").exists()
        || Command::new("pacman")
            .args(["-Q", "omarchy"])
            .output()
            .is_ok_and(|o| o.status.success());
    detect(&os, omarchy)
        .ok_or_else(|| "Borea doesn't recognise this distribution (/etc/os-release).".into())
}

fn root() -> bool {
    fs::read_to_string("/proc/self/status").ok().is_some_and(|s| {
        s.lines()
            .find_map(|l| l.strip_prefix("Uid:"))
            .and_then(|u| u.split_whitespace().nth(1))
            .is_some_and(|euid| euid == "0")
    })
}

fn installed() -> Result<BTreeSet<String>> {
    let out = Command::new("pacman").arg("-Qq").output()?;
    Ok(String::from_utf8_lossy(&out.stdout).lines().map(str::to_owned).collect())
}

fn run(program: &str, args: &[String]) -> Result {
    let status = Command::new(program).args(args).status()?;
    if !status.success() {
        return Err(format!("{program} {} did not succeed ({status}).", args.join(" ")).into());
    }
    Ok(())
}

/// `borea extras`: what this system is offered, and what Borea has added.
pub fn list(out: &mut impl Write) -> Result {
    let catalog = Catalog::bundled()?;
    let distro = system()?;
    let added = record();
    let present = if pacman(distro) { installed().unwrap_or_default() } else { BTreeSet::new() };
    writeln!(out, "Extras for {distro}:")?;
    for o in catalog.extras.on(&DistroId(distro.to_owned())) {
        let state = if o.provision.built_in {
            "built in"
        } else if added.contains_key(&o.extra.id) {
            "added by Borea"
        } else if !o.provision.packages.is_empty()
            && o.provision.packages.iter().all(|p| present.contains(p))
        {
            "installed"
        } else {
            "not installed"
        };
        writeln!(out, "  {:<9} {:<15} {}", o.extra.id, state, o.describe())?;
    }
    if !pacman(distro) {
        writeln!(
            out,
            "\n`borea install` comes to {distro} later; Arch, CachyOS and Omarchy first."
        )?;
    }
    Ok(())
}

/// `borea install EXTRA… [--yes]`.
pub fn install(out: &mut impl Write, args: &[&str]) -> Result {
    let yes = args.contains(&"--yes");
    let ids: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with("--")).collect();
    if ids.is_empty() {
        return Err("Name an extra, e.g. borea install gaming. `borea extras` lists them.".into());
    }
    let catalog = Catalog::bundled()?;
    let distro = system()?;
    let mut plan = plan(&catalog, distro, &ids)?;
    if !pacman(distro) {
        return Err(format!("`borea install` doesn't support {distro} yet.").into());
    }
    if plan.extras.is_empty() {
        writeln!(out, "Nothing to add: built in on {distro}.")?;
        return Ok(());
    }
    if !root() {
        return Err(format!("Run it as root: sudo borea install {}", ids.join(" ")).into());
    }
    if plan.multilib {
        let conf = fs::read_to_string("/etc/pacman.conf")?;
        if let Some(new) = with_multilib(&conf) {
            writeln!(out, "Turning on the multilib repository (/etc/pacman.conf).")?;
            fs::write("/etc/pacman.conf", new)?;
        }
        let drivers: Vec<String> =
            graphics_vendors().iter().filter_map(|v| lib32_vulkan(v)).map(str::to_owned).collect();
        if let Some((_, packages)) = plan.extras.iter_mut().find(|(id, _)| id == "gaming") {
            packages.extend(drivers);
        }
    }
    let before = installed()?;
    let packages = plan.packages();
    writeln!(out, "Adding {}: {}", ids.join(", "), packages.join(" "))?;
    // A full sync with the upgrade, as Arch requires: never a partial upgrade.
    let mut pacman_args = vec!["-Syu".to_owned(), "--needed".to_owned()];
    if yes {
        pacman_args.push("--noconfirm".into());
    }
    pacman_args.extend(packages);
    run("pacman", &pacman_args)?;
    let after = installed()?;
    let mut record = record();
    for (id, packages) in &plan.extras {
        let added: Vec<String> = packages
            .iter()
            .filter(|p| !before.contains(*p) && after.contains(*p))
            .cloned()
            .collect();
        let entry = record.entry(id.clone()).or_default();
        for p in added {
            if !entry.contains(&p) {
                entry.push(p);
            }
        }
    }
    save(&record)?;
    writeln!(out, "Done.")?;
    Ok(())
}

/// `borea remove EXTRA… [--yes]`: only the packages Borea added for it.
pub fn remove(out: &mut impl Write, args: &[&str]) -> Result {
    let yes = args.contains(&"--yes");
    let ids: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with("--")).collect();
    if ids.is_empty() {
        return Err("Name an extra, e.g. borea remove gaming.".into());
    }
    let distro = system()?;
    if !pacman(distro) {
        return Err(format!("`borea remove` doesn't support {distro} yet.").into());
    }
    let mut record = record();
    let present = installed()?;
    let mut packages = Vec::new();
    for id in &ids {
        let Some(added) = record.get(*id) else {
            return Err(format!(
                "Borea didn't add {id}, so it leaves it alone. Remove packages with pacman."
            )
            .into());
        };
        packages.extend(added.iter().filter(|p| present.contains(*p)).cloned());
    }
    if !root() {
        return Err(format!("Run it as root: sudo borea remove {}", ids.join(" ")).into());
    }
    if !packages.is_empty() {
        writeln!(out, "Removing {}: {}", ids.join(", "), packages.join(" "))?;
        let mut pacman_args = vec!["-Rns".to_owned()];
        if yes {
            pacman_args.push("--noconfirm".into());
        }
        pacman_args.extend(packages);
        run("pacman", &pacman_args)?;
    }
    for id in &ids {
        record.remove(*id);
    }
    save(&record)?;
    writeln!(out, "Done.")?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn recognises_arch_family_systems() {
        assert_eq!(detect("NAME=\"Arch Linux\"\nID=arch\n", false), Some("arch"));
        assert_eq!(detect("ID=arch\n", true), Some("omarchy"));
        assert_eq!(detect("ID=cachyos\nID_LIKE=arch\n", false), Some("cachyos"));
        assert_eq!(detect("ID=endeavouros\nID_LIKE=arch\n", false), Some("arch"));
        assert_eq!(detect("ID=\"opensuse-tumbleweed\"\n", false), Some("tumbleweed"));
        assert_eq!(detect("ID=gentoo\n", false), None);
        assert!(pacman("omarchy") && !pacman("fedora"));
    }

    #[test]
    fn plans_from_the_catalog_and_refuses_what_it_does_not_offer() {
        let catalog = Catalog::bundled().unwrap();
        let p = plan(&catalog, "omarchy", &["printing", "gaming", "printing"]).unwrap();
        assert_eq!(p.extras.len(), 2, "each once");
        assert!(p.multilib);
        assert!(p.packages().contains(&"cups".to_owned()));
        let printing = plan(&catalog, "arch", &["printing"]).unwrap();
        assert!(!printing.multilib);
        let unknown = plan(&catalog, "arch", &["rootkit"]).unwrap_err().to_string();
        assert!(unknown.contains("gaming"), "{unknown}");
    }

    #[test]
    fn multilib_is_uncommented_or_added_once() {
        let commented = "[core]\nInclude = x\n\n#[multilib]\n#Include = /etc/pacman.d/mirrorlist\n";
        let on = with_multilib(commented).unwrap();
        assert!(on.contains("\n[multilib]\nInclude = /etc/pacman.d/mirrorlist\n"), "{on}");
        assert_eq!(with_multilib(&on), None);
        let absent = with_multilib("[core]\nInclude = x\n").unwrap();
        assert!(absent.ends_with("[multilib]\nInclude = /etc/pacman.d/mirrorlist\n"));
    }

    #[test]
    fn vulkan_driver_follows_the_graphics_vendor() {
        assert_eq!(lib32_vulkan("0x8086\n"), Some("lib32-vulkan-intel"));
        assert_eq!(lib32_vulkan("0x10de"), Some("lib32-nvidia-utils"));
        assert_eq!(lib32_vulkan("0x1234"), None);
    }

    #[test]
    fn record_round_trips() {
        let mut r = BTreeMap::new();
        r.insert(
            "printing".to_owned(),
            vec!["cups".to_owned(), "system-config-printer".to_owned()],
        );
        r.insert("flatpak".to_owned(), vec![]);
        assert_eq!(parse_record(&write_record(&r)), r);
    }
}
