//! Host-independent storage and account preflight shared by target adapters.
//! This module never writes a disk or invokes a shell.

use std::path::Path;

use borea_core::{Architecture, Fact, FirmwareMode};
use borea_hardware::{DiskUsage, HardwareSnapshot};

use crate::plan::{InstallRequest, TargetDisk};

pub const ESP_MIB: u64 = 2048;

/// First supported layout: explicitly chosen whole disk, GPT, EFI + ext4.
/// Unknown or mounted disks are never inferred to be disposable.
pub fn preflight(
    request: &InstallRequest,
    hw: &HardwareSnapshot,
    root: &Path,
    minimum_bytes: u64,
) -> Result<TargetDisk, String> {
    if !hw.cpu.known().is_some_and(|cpu| cpu.architecture == Architecture::X86_64) {
        return Err("installation currently requires a known x86-64 CPU".into());
    }
    match &hw.firmware {
        Fact::Known(FirmwareMode::Uefi { bits: Some(64) | None }) => {}
        _ => return Err("installation currently requires 64-bit UEFI".into()),
    }
    if hw.secure_boot != Fact::Known(false) {
        return Err("Secure Boot must be confirmed off before installation".into());
    }
    for (field, value) in [
        ("hostname", &request.hostname),
        ("username", &request.username),
        ("timezone", &request.timezone),
        ("locale", &request.locale),
        ("keymap", &request.keymap),
    ] {
        check_setting(field, value)?;
    }
    let zones = root.join("usr/share/zoneinfo");
    if zones.is_dir() && !zones.join(&request.timezone).is_file() {
        return Err(format!("unknown time zone `{}`", request.timezone));
    }
    let disk = hw
        .disks
        .iter()
        .find(|d| d.name == request.disk)
        .ok_or_else(|| format!("no disk `{}` here", request.disk))?;
    if disk.usage != DiskUsage::Free {
        return Err(format!("{} can't be erased: {}", disk.name, disk.usage));
    }
    if disk.size_bytes < minimum_bytes {
        return Err(format!("{} needs at least {} GiB", disk.name, minimum_bytes >> 30));
    }
    let stable_path = crate::disk::stable_path(root, &disk.name)
        .ok_or_else(|| format!("{} has no stable name under /dev/disk", disk.name))?;
    Ok(TargetDisk {
        name: disk.name.clone(),
        stable_path,
        size_bytes: disk.size_bytes,
        model: disk.model.clone(),
    })
}

pub fn check_setting(field: &str, value: &str) -> Result<(), String> {
    let ok = match field {
        "hostname" => {
            (1..=63).contains(&value.len())
                && value.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                && !value.starts_with('-')
                && !value.ends_with('-')
        }
        "username" => {
            (1..=32).contains(&value.len())
                && value.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
                && value
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-".contains(&c))
                && !["root", "daemon", "bin", "sys", "sync", "nobody"].contains(&value)
        }
        "timezone" => {
            !value.is_empty()
                && !value.contains("..")
                && !value.starts_with('/')
                && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"/_-+".contains(&c))
        }
        "locale" | "keymap" => {
            !value.is_empty()
                && !value.starts_with('-')
                && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-@".contains(&c))
        }
        _ => false,
    };
    if ok { Ok(()) } else { Err(format!("`{value}` is not a valid {field}")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_and_locale_inputs_cannot_be_options_or_paths() {
        for (field, bad) in [
            ("username", "root"),
            ("username", "daemon"),
            ("username", "a:b"),
            ("hostname", "-host"),
            ("hostname", "host-"),
            ("timezone", "../../etc"),
            ("timezone", "/etc"),
            ("locale", "--help"),
            ("keymap", "us\nKEYMAP=bad"),
        ] {
            assert!(check_setting(field, bad).is_err(), "{field}: {bad}");
        }
        for (field, good) in [
            ("username", "luke"),
            ("hostname", "borea-pc"),
            ("timezone", "Europe/London"),
            ("locale", "en_GB.UTF-8"),
            ("keymap", "uk"),
        ] {
            check_setting(field, good).unwrap();
        }
    }
}
