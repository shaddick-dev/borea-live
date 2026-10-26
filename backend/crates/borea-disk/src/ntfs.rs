//! Native NTFS inspection uses only commands which open the volume read-only.
use crate::Partition;
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
};
fn output(tool: &str, args: &[&str], path: &Path) -> Option<std::process::Output> {
    Command::new(borea_platform::live_program(tool).ok()?)
        .args(args)
        .arg(path)
        .env("LC_ALL", "C")
        .output()
        .ok()
}
fn volume_flags(text: &str) -> Option<u16> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Volume Flags: 0x")
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|hex| u16::from_str_radix(hex, 16).ok())
    })
}
fn health(text: &str) -> Option<bool> {
    let flags = volume_flags(text)?;
    let lines = text.lines().map(str::trim).collect::<Vec<_>>();
    if lines.contains(&"Restart state: DIRTY") {
        return Some(true);
    }
    if lines.contains(&"Restart state: CLEAN") || lines.contains(&"No restart page") {
        Some(flags & 1 != 0)
    } else {
        None
    }
}
fn journal(path: &Path) -> Option<bool> {
    const LIMIT: u64 = 256 * 1024 * 1024;
    let mut child = Command::new(borea_platform::live_program("/usr/bin/ntfscat").ok()?)
        .arg(path)
        .arg("/$LogFile")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("LC_ALL", "C")
        .spawn()
        .ok()?;
    let mut bytes = Vec::new();
    let read = child.stdout.take().map(|stdout| stdout.take(LIMIT + 1).read_to_end(&mut bytes));
    if bytes.len() as u64 > LIMIT || !matches!(read, Some(Ok(_))) {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    if !child.wait().ok()?.success() {
        return None;
    }
    crate::journal::dirty(&bytes)
}
fn hibernated(path: &Path) -> Option<bool> {
    let listing = output("/usr/bin/ntfsls", &["--all", "--system", "--path", "/"], path)?;
    if !listing.status.success() {
        return None;
    }
    let listing = String::from_utf8_lossy(&listing.stdout);
    let file = listing.lines().find(|name| name.eq_ignore_ascii_case("hiberfil.sys"));
    let Some(file) = file else {
        return Some(false);
    };
    let mut child = Command::new(borea_platform::live_program("/usr/bin/ntfscat").ok()?)
        .arg(path)
        .arg(format!("/{file}"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("LC_ALL", "C")
        .spawn()
        .ok()?;
    let mut header = Vec::new();
    let read = child.stdout.take().map(|stdout| stdout.take(4096).read_to_end(&mut header));
    // Do not read a many-gigabyte hibernation file into memory or wait for its producer.
    let _ = child.kill();
    let _ = child.wait();
    if !matches!(read, Some(Ok(_))) {
        return None;
    }
    Some(header.len() < 4096 || header.iter().any(|byte| *byte != 0))
}
pub(crate) fn inspect(part: &mut Partition) {
    let Some(info) = output("/usr/bin/ntfsinfo", &["--mft"], &part.path) else {
        return;
    };
    if !info.status.success() {
        return;
    }
    let text = String::from_utf8_lossy(&info.stdout);
    let Some(flags) = volume_flags(&text) else {
        return;
    };
    let Some(log_dirty) = health(&text).or_else(|| journal(&part.path)) else {
        return;
    };
    let dirty = flags & 1 != 0 || log_dirty;
    part.ntfs_dirty = dirty;
    let Some(hibernated) = hibernated(&part.path) else {
        return;
    };
    part.ntfs_hibernated = hibernated;
    part.ntfs_checked = true;
    if dirty || hibernated {
        return;
    }
    if let Some(out) = output("/usr/sbin/ntfsresize", &["--info", "--no-progress-bar"], &part.path)
    {
        if out.status.success() {
            part.ntfs_min_size = String::from_utf8_lossy(&out.stdout).lines().find_map(|line| {
                line.strip_prefix("You might resize at ")
                    .and_then(|s| s.split_whitespace().next())
                    .and_then(|s| s.parse().ok())
            });
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_volume_and_restart_state_must_both_be_known_and_clean() {
        assert_eq!(health("\tVolume Flags: 0x0000\n\tRestart state: CLEAN\n"), Some(false));
        assert_eq!(health("\tVolume Flags: 0x0001 DIRTY\n\tRestart state: CLEAN\n"), Some(true));
        assert_eq!(health("\tVolume Flags: 0x0000\n\tRestart state: DIRTY\n"), Some(true));
        assert_eq!(health("\tVolume Flags: 0x0000\n\tNo restart page\n"), Some(false));
        assert_eq!(health("\tVolume Flags: 0x0000\n"), None);
        assert_eq!(health("unreadable metadata"), None);
    }
    #[test]
    #[ignore = "requires a marked disposable regular NTFS image and native ntfsprogs"]
    #[allow(clippy::unwrap_used)]
    fn native_probe_preserves_every_byte_and_detects_active_hibernation() {
        use sha2::{Digest, Sha256};
        let path = std::path::PathBuf::from(std::env::var("BOREA_TEST_NTFS_IMAGE").unwrap());
        assert!(path.starts_with("/fixtures"));
        assert!(
            std::fs::read_to_string("/fixtures/.borea-ntfs-fixture").unwrap().trim()
                == "BOREA_TEST_NTFS"
        );
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert!(
            metadata.is_file() && !metadata.is_symlink() && metadata.len() <= 512 * 1024 * 1024
        );
        let hash = || {
            let mut file = std::fs::File::open(&path).unwrap();
            let mut digest = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop {
                let n = file.read(&mut buffer).unwrap();
                if n == 0 {
                    break;
                }
                digest.update(&buffer[..n]);
            }
            digest.finalize()
        };
        let check = |hibernated: bool| {
            let before = hash();
            let mut part = Partition {
                path: path.clone(),
                number: 1,
                uuid: String::new(),
                start: 0,
                size: metadata.len(),
                fs: "ntfs".into(),
                efi: false,
                free: None,
                bitlocker: false,
                ntfs_checked: false,
                ntfs_hibernated: false,
                ntfs_dirty: false,
                ntfs_min_size: None,
            };
            inspect(&mut part);
            assert_eq!(before, hash(), "Read-only probing modified the NTFS image");
            assert!(part.ntfs_checked, "Native NTFS health was not decoded: {part:?}");
            assert!(!part.ntfs_dirty);
            assert_eq!(part.ntfs_hibernated, hibernated);
            if !hibernated {
                assert!(part.ntfs_min_size.is_some());
            }
        };
        check(false);
        for active in [false, true] {
            let mut bytes = vec![0u8; 4096];
            if active {
                bytes[..4].copy_from_slice(b"hibr");
            }
            std::fs::write("/fixtures/hiber-header", bytes).unwrap();
            let status = Command::new(borea_platform::live_program("/usr/bin/ntfscp").unwrap())
                .arg(&path)
                .args(["/fixtures/hiber-header", "/hiberfil.sys"])
                .status()
                .unwrap();
            assert!(status.success());
            check(active);
        }
    }
}
