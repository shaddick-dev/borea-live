//! The Arch Linux adapter: its tools come from Arch's signed bootstrap archive,
//! never from the live system, and it installs with Arch's own pacstrap.

use std::fs;
use std::path::{Path, PathBuf};

use borea_catalog::Catalog;
use borea_core::{DistroId, Fact, FirmwareMode};
use borea_hardware::{DiskUsage, HardwareSnapshot, Microcode};

use crate::fetch;
use crate::plan::{
    Artifact, Boot, InstallPlan, InstallRequest, Layout, SCHEMA, System, TargetDisk,
};

/// Arch Linux's release signing key (Pierre Schmitz), fetched by WKD and checked
/// against the fingerprint archlinux.org/download lists.
pub const RELEASE_KEY: &str = include_str!("../keys/arch-release-key.asc");
pub const RELEASE_KEY_FINGERPRINT: &str = "3E80CA1A8B89F69CBA57D98A76A5EF9054449A5C";

/// Arch's own geographically balanced mirror.
pub const MIRROR: &str = "https://geo.mirror.pkgbuild.com";

/// The bootstrap archive is about 130 MB; refuse anything far beyond that.
const BOOTSTRAP_LIMIT: u64 = 512 << 20;

/// Editions whose whole install has been run and booted in a VM. Others stay
/// unplannable until they have been too.
pub const TESTED_EDITIONS: &[&str] = &["minimal"];

/// The smallest disk for a Minimal install: base, kernel, firmware, and room.
pub const MIN_DISK: u64 = 8 << 30;
pub const ESP_MIB: u64 = 2048;

/// Find the newest bootstrap archive, download it to `cache`, and check its
/// signature and published checksum. Returns the pinned artifact and its file.
pub fn resolve_bootstrap(
    cache: &Path,
    progress: impl FnMut(u64),
) -> Result<(Artifact, PathBuf), String> {
    let sums = fetch::text(&format!("{MIRROR}/iso/latest/sha256sums.txt"))?;
    let (sha256, name, release) = latest_bootstrap(&sums)
        .ok_or("the mirror lists no dated bootstrap archive in iso/latest")?;
    let url = format!("{MIRROR}/iso/{release}/{name}");
    fs::create_dir_all(cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    let file = cache.join(&name);

    let cached = fetch::sha256_file(&file).is_ok_and(|have| have == sha256);
    if !cached {
        let (got, _) = fetch::download(&url, &file, BOOTSTRAP_LIMIT, progress)?;
        if got != sha256 {
            return Err(format!("{name}: SHA-256 {got} is not the published {sha256}"));
        }
    }
    let signature = fetch::bytes(&format!("{url}.sig"))?;
    let signed_by = fetch::verify_signature(RELEASE_KEY, &signature, &file)?;
    if signed_by != RELEASE_KEY_FINGERPRINT {
        return Err(format!("{name} is signed by {signed_by}, not Arch's release key"));
    }
    let size = fs::metadata(&file).map_err(|e| e.to_string())?.len();
    Ok((Artifact { release, url, sha256, size, signed_by }, file))
}

/// The dated bootstrap line of a `sha256sums.txt`: (sha256, file name, release).
fn latest_bootstrap(sums: &str) -> Option<(String, String, String)> {
    sums.lines().find_map(|line| {
        let (sha, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim();
        let release = name.strip_prefix("archlinux-bootstrap-")?.strip_suffix("-x86_64.tar.zst")?;
        (sha.len() == 64 && release.split('.').count() == 3)
            .then(|| (sha.to_owned(), name.to_owned(), release.to_owned()))
    })
}

/// Everything [`plan`] checks that needs no download: run it first, so a
/// mistake shows before a 130 MB fetch.
pub fn check(
    request: &InstallRequest,
    catalog: &Catalog,
    hw: &HardwareSnapshot,
    root: &Path,
) -> Result<(), String> {
    plan(request, catalog, hw, root, Artifact::default()).map(|_| ())
}

/// Plan an Arch install of `request` on this machine. Read-only: `root` is `/`,
/// or a fixture in tests; `bootstrap` is the already verified archive.
pub fn plan(
    request: &InstallRequest,
    catalog: &Catalog,
    hw: &HardwareSnapshot,
    root: &Path,
    bootstrap: Artifact,
) -> Result<InstallPlan, String> {
    let distro = catalog
        .get(&request.distro)
        .filter(|d| d.id == DistroId("arch".into()))
        .ok_or_else(|| format!("this adapter installs Arch Linux, not `{}`", request.distro))?;
    let edition = distro
        .editions
        .iter()
        .find(|e| e.id == request.edition)
        .ok_or_else(|| format!("Arch Linux has no edition `{}`", request.edition))?;
    if !TESTED_EDITIONS.contains(&edition.id.as_str()) {
        return Err(format!(
            "{} is not installable yet: only {} has been installed and booted end to end",
            edition.name,
            TESTED_EDITIONS.join(", ")
        ));
    }

    match &hw.firmware {
        Fact::Known(FirmwareMode::Uefi { bits: Some(64) | None }) => {}
        Fact::Known(FirmwareMode::Uefi { .. }) => {
            return Err("32-bit UEFI is not supported yet".into());
        }
        Fact::Known(FirmwareMode::Bios) => {
            return Err("this machine started in legacy BIOS mode; installing needs UEFI".into());
        }
        Fact::Unknown { reason } => return Err(format!("firmware mode unknown: {reason}")),
    }
    if hw.secure_boot == Fact::Known(true) {
        return Err("Secure Boot is on; turn it off in firmware setup to install for now".into());
    }

    let disk = hw
        .disks
        .iter()
        .find(|d| d.name == request.disk)
        .ok_or_else(|| format!("no disk `{}` here", request.disk))?;
    if disk.usage != DiskUsage::Free {
        return Err(format!("{} can't be erased: {}", disk.name, disk.usage));
    }
    if disk.size_bytes < MIN_DISK {
        return Err(format!(
            "{} holds {} GB; Arch Linux Minimal needs at least {} GB",
            disk.name,
            disk.size_bytes / 1_000_000_000,
            MIN_DISK / 1_000_000_000
        ));
    }
    let stable_path = crate::disk::stable_path(root, &disk.name)
        .ok_or_else(|| format!("{} has no stable name under /dev/disk", disk.name))?;

    let microcode = hw.cpu.known().and_then(|c| c.microcode());
    let mut packages: Vec<String> =
        ["base", "linux", "linux-firmware", "sudo", "networkmanager", "limine", "efibootmgr"]
            .map(String::from)
            .into();
    let microcode_package = match microcode {
        Some(Microcode::Intel) => Some("intel-ucode"),
        Some(Microcode::Amd) => Some("amd-ucode"),
        None => None,
    };
    packages.extend(microcode_package.map(String::from));
    for id in &request.extras {
        let offered = catalog
            .extras
            .on(&distro.id)
            .find(|o| o.extra.id == *id)
            .ok_or_else(|| format!("Arch Linux offers no extra `{id}`"))?;
        if let Some(needs) = &offered.provision.needs {
            return Err(format!(
                "{} needs {needs}, which this installer can't enable yet",
                offered.extra.name
            ));
        }
        packages.extend(offered.provision.packages.iter().cloned());
    }
    packages.dedup();

    for (field, value) in [
        ("hostname", &request.hostname),
        ("username", &request.username),
        ("timezone", &request.timezone),
        ("locale", &request.locale),
        ("keymap", &request.keymap),
    ] {
        check_setting(field, value)?;
    }
    if !root.join("usr/share/zoneinfo").join(&request.timezone).is_file()
        && root.join("usr/share/zoneinfo").is_dir()
    {
        return Err(format!("unknown time zone `{}`", request.timezone));
    }

    Ok(InstallPlan {
        schema: SCHEMA,
        distro: distro.id.to_string(),
        edition: edition.id.clone(),
        disk: TargetDisk {
            name: disk.name.clone(),
            stable_path,
            size_bytes: disk.size_bytes,
            model: disk.model.clone(),
        },
        layout: Layout { esp_mib: ESP_MIB, root_fs: "ext4".into() },
        bootstrap,
        mirror: MIRROR.into(),
        packages,
        system: System {
            hostname: request.hostname.clone(),
            timezone: request.timezone.clone(),
            locale: request.locale.clone(),
            keymap: request.keymap.clone(),
            username: request.username.clone(),
        },
        boot: Boot {
            loader: "limine".into(),
            microcode: microcode_package.map(|p| format!("{}.img", p)),
            kernel_args: request.kernel_args.clone(),
        },
        services: vec!["NetworkManager.service".into()],
    })
}

/// Settings end up in files and command arguments: allow only what each can hold.
fn check_setting(field: &str, value: &str) -> Result<(), String> {
    let ok = match field {
        "hostname" => {
            (1..=63).contains(&value.len())
                && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !value.starts_with('-')
        }
        "username" => {
            (1..=32).contains(&value.len())
                && value.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                && value
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-".contains(c))
                && value != "root"
        }
        "timezone" => {
            !value.is_empty()
                && !value.contains("..")
                && !value.starts_with('/')
                && value.chars().all(|c| c.is_ascii_alphanumeric() || "/_-+".contains(c))
        }
        _ => {
            !value.is_empty()
                && value.chars().all(|c| c.is_ascii_alphanumeric() || "._-@".contains(c))
        }
    };
    if ok { Ok(()) } else { Err(format!("`{value}` is not a valid {field}")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use borea_core::Architecture;
    use borea_hardware::{Cpu, Disk, DiskTransport, Scanner};
    use borea_testkit::FakeRoot;

    const SUMS: &str = "\
684ded26c63240ff4a41e8c25ee84ea6da233f557364821f13d12c2b0a9059a5  archlinux-2026.10.01-x86_64.iso
0c9c97b7a58a8a023c9e07f84cb307a9e8006e65d8fc4c17b83ddc8010b45c10  archlinux-bootstrap-2026.10.01-x86_64.tar.zst
0c9c97b7a58a8a023c9e07f84cb307a9e8006e65d8fc4c17b83ddc8010b45c10  archlinux-bootstrap-x86_64.tar.zst
";

    #[test]
    fn picks_the_dated_bootstrap() {
        let (sha, name, release) = latest_bootstrap(SUMS).unwrap();
        assert_eq!(release, "2026.10.01");
        assert_eq!(name, "archlinux-bootstrap-2026.10.01-x86_64.tar.zst");
        assert!(sha.starts_with("0c9c97b7"));
    }

    fn artifact() -> Artifact {
        Artifact {
            release: "2026.10.01".into(),
            url: format!("{MIRROR}/iso/2026.10.01/archlinux-bootstrap-2026.10.01-x86_64.tar.zst"),
            sha256: "0c9c".into(),
            size: 127_355_946,
            signed_by: RELEASE_KEY_FINGERPRINT.into(),
        }
    }

    fn request() -> InstallRequest {
        InstallRequest {
            distro: "arch".into(),
            edition: "minimal".into(),
            disk: "vda".into(),
            hostname: "wind".into(),
            username: "luke".into(),
            timezone: "Europe/London".into(),
            locale: "en_GB.UTF-8".into(),
            keymap: "uk".into(),
            extras: vec![],
            kernel_args: vec![],
        }
    }

    fn machine(root: &FakeRoot) -> HardwareSnapshot {
        let mut hw = Scanner::new(root.path()).scan();
        hw.firmware = Fact::Known(FirmwareMode::Uefi { bits: Some(64) });
        hw.secure_boot = Fact::Known(false);
        hw.cpu = Fact::Known(Cpu {
            architecture: Architecture::X86_64,
            model: None,
            vendor: Some("GenuineIntel".into()),
            x86_level: None,
            logical_cores: 2,
        });
        hw.disks = vec![Disk {
            name: "vda".into(),
            model: None,
            size_bytes: 20 << 30,
            removable: false,
            rotational: None,
            transport: DiskTransport::Virtio,
            partitions: vec![],
            usage: DiskUsage::Free,
        }];
        hw
    }

    fn fixture() -> FakeRoot {
        let root = FakeRoot::new();
        root.link("dev/disk/by-path/pci-0000:00:04.0", "dev/vda")
            .file("usr/share/zoneinfo/Europe/London", "TZif");
        root
    }

    #[test]
    fn plans_minimal_on_a_free_disk() {
        let root = fixture();
        let catalog = Catalog::bundled().unwrap();
        let plan = plan(&request(), &catalog, &machine(&root), root.path(), artifact()).unwrap();
        assert_eq!(plan.disk.stable_path, "/dev/disk/by-path/pci-0000:00:04.0");
        assert!(plan.packages.contains(&"intel-ucode".into()));
        assert_eq!(plan.boot.microcode.as_deref(), Some("intel-ucode.img"));
        assert_eq!(plan.digest(), plan.clone().digest(), "the digest is stable");
        assert_eq!(InstallPlan::from_json(&plan.to_json()).unwrap(), plan);
        assert!(plan.review()[0].starts_with("Erase vda"));
    }

    #[test]
    fn refuses_what_it_cannot_do_yet() {
        let root = fixture();
        let catalog = Catalog::bundled().unwrap();
        let hw = machine(&root);
        let refuse = |change: &dyn Fn(&mut InstallRequest, &mut HardwareSnapshot)| {
            let (mut request, mut hw) = (request(), hw.clone());
            change(&mut request, &mut hw);
            plan(&request, &catalog, &hw, root.path(), artifact()).unwrap_err()
        };
        assert!(refuse(&|r, _| r.edition = "kde-plasma".into()).contains("not installable yet"));
        assert!(refuse(&|_, h| h.firmware = Fact::Known(FirmwareMode::Bios)).contains("UEFI"));
        assert!(refuse(&|_, h| h.disks[0].usage = DiskUsage::HostRoot).contains("can't be erased"));
        assert!(refuse(&|_, h| h.disks[0].size_bytes = 4 << 30).contains("at least"));
        assert!(refuse(&|r, _| r.disk = "vdb".into()).contains("no disk"));
        assert!(refuse(&|r, _| r.extras = vec!["gaming".into()]).contains("multilib"));
        assert!(refuse(&|r, _| r.username = "Root; rm".into()).contains("not a valid username"));
        assert!(
            refuse(&|r, _| r.timezone = "../../etc/shadow".into()).contains("not a valid timezone")
        );
        assert!(refuse(&|r, _| r.timezone = "Mars/Olympus".into()).contains("unknown time zone"));
    }

    #[test]
    fn plain_extras_add_their_packages() {
        let root = fixture();
        let catalog = Catalog::bundled().unwrap();
        let mut request = request();
        request.extras = vec!["printing".into()];
        let plan = plan(&request, &catalog, &machine(&root), root.path(), artifact()).unwrap();
        assert!(plan.packages.contains(&"cups".into()), "{:?}", plan.packages);
    }
}
