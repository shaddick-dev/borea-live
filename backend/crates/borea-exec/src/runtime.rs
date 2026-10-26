//! Target helpers must not depend on the live system's libc version.
use borea_core::{Hint, hint};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
pub(crate) fn fault_suggestion(message: &str, stderr: &[String]) -> &'static str {
    let text = format!("{message}\n{}", stderr.join("\n")).to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| text.contains(needle));
    if has(&["no space left", "disk full", "not enough free", "insufficient space"]) {
        "Free space on the installation target before retrying the failed package action."
    } else if has(&["signature", "no_pubkey", "public key", "keyring", "not signed"]) {
        "Check the upstream signing key and refresh its trusted keyring before retrying."
    } else if has(&[
        "unable to locate package",
        "target not found",
        "no match for argument",
        "nothing provides",
        "no provider of",
    ]) {
        "Report this package name and distribution as a catalog bug; do not substitute an unsigned source."
    } else if has(&[
        "could not resolve",
        "network is unreachable",
        "connection timed out",
        "failed to fetch",
        "http error",
        "certificate",
        "download failed",
    ]) {
        "Check the internet connection, system clock and mirror access, then retry the failed action."
    } else {
        "Inspect the redacted log for the failed command before continuing."
    }
}
/// FAT stores permissions in its mount masks, not individual inode mode bits.
pub(crate) fn set_mode(path: &Path, mode: u32) -> Result<(), Hint> {
    use std::os::unix::fs::PermissionsExt;
    if nix::sys::statfs::statfs(path)
        .map_err(|e| hint("permissions", e.to_string()))?
        .filesystem_type()
        != nix::sys::statfs::MSDOS_SUPER_MAGIC
    {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| hint("permissions", e.to_string()))?;
    }
    Ok(())
}
pub(crate) fn capture_efi(
    root: &Path,
) -> Result<std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>, Hint> {
    let mut files = std::collections::BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    let mut total = 0u64;
    while let Some(path) = pending.pop() {
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if path == root && error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(hint("efi", error.to_string())),
        };
        if metadata.is_symlink() {
            return Err(hint("efi", "EFI fallback preservation refuses symlinks."));
        }
        if path == root && !metadata.is_dir() {
            return Err(hint("efi", "EFI/BOOT is not a directory."));
        }
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).map_err(|e| hint("efi", e.to_string()))? {
                pending.push(entry.map_err(|e| hint("efi", e.to_string()))?.path());
                if pending.len() + files.len() > 4096 {
                    return Err(hint(
                        "efi",
                        "The EFI fallback tree exceeds the preservation limit.",
                    ));
                }
            }
        } else if metadata.is_file() {
            total = total.saturating_add(metadata.len());
            if total > 128 * 1024 * 1024 {
                return Err(hint(
                    "efi",
                    "EFI fallback files exceed the 128 MiB RAM preservation limit.",
                ));
            }
            let mut bytes = Vec::new();
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(nix::libc::O_NOFOLLOW)
                .open(&path)
                .and_then(|file| file.take(metadata.len() + 1).read_to_end(&mut bytes))
                .map_err(|e| hint("efi", e.to_string()))?;
            if bytes.len() as u64 != metadata.len() {
                return Err(hint("efi", "An EFI fallback file changed during capture."));
            }
            let relative =
                path.strip_prefix(root).map_err(|e| hint("efi", e.to_string()))?.to_path_buf();
            files.insert(relative, bytes);
        } else {
            return Err(hint("efi", "An EFI fallback entry is not a file or directory."));
        }
    }
    Ok(files)
}
fn error() -> Hint {
    hint(
        "runtime",
        "Install a statically linked x86-64 borea-firstboot helper (build with x86_64-unknown-linux-musl).",
    )
}
pub fn static_helper(path: &Path) -> Result<(), Hint> {
    let mut file = File::open(path).map_err(|_| error())?;
    let size = file.metadata().map_err(|_| error())?.len();
    let mut header = [0u8; 64];
    file.read_exact(&mut header).map_err(|_| error())?;
    if &header[..4] != b"\x7fELF"
        || header[4] != 2
        || header[5] != 1
        || header[18..20] != [62, 0]
        || ![2, 3].contains(&u16::from_le_bytes([header[16], header[17]]))
    {
        return Err(error());
    }
    let offset = u64::from_le_bytes(header[32..40].try_into().map_err(|_| error())?);
    let stride = u64::from(u16::from_le_bytes([header[54], header[55]]));
    let count = u64::from(u16::from_le_bytes([header[56], header[57]]));
    if stride < 56
        || count == 0
        || count > 4096
        || offset.checked_add(stride * count).is_none_or(|end| end > size)
    {
        return Err(error());
    }
    for index in 0..count {
        file.seek(SeekFrom::Start(offset + stride * index)).map_err(|_| error())?;
        let mut kind = [0u8; 4];
        file.read_exact(&mut kind).map_err(|_| error())?;
        if u32::from_le_bytes(kind) == 3 {
            return Err(error());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_helper_rejects_live_libc_interpreter() {
        let mut data = vec![0u8; 120];
        data[..4].copy_from_slice(b"\x7fELF");
        data[4] = 2;
        data[5] = 1;
        data[16] = 2;
        data[18] = 62;
        data[32] = 64;
        data[54] = 56;
        data[56] = 1;
        data[64] = 1;
        let path = std::env::temp_dir().join(format!("borea-elf-fixture-{}", std::process::id()));
        assert!(std::fs::write(&path, &data).is_ok());
        assert!(static_helper(&path).is_ok());
        data[64] = 3;
        assert!(std::fs::write(&path, &data).is_ok());
        assert!(static_helper(&path).is_err());
        let _ = std::fs::remove_file(path);
    }
}

/// Constructed only after Session has created a distinct private mount namespace.
pub(crate) struct PrivateNamespace {
    owned: std::path::PathBuf,
}
impl PrivateNamespace {
    pub(crate) fn capture(previous: &Path) -> Result<Self, Hint> {
        let owned = std::fs::read_link("/proc/thread-self/ns/mnt")
            .map_err(|e| hint("namespace", e.to_string()))?;
        if owned == previous {
            return Err(hint(
                "namespace",
                "The installer did not acquire a private mount namespace.",
            ));
        }
        Ok(Self { owned })
    }
    pub(crate) fn stop_children(&self) -> Result<(), Hint> {
        if std::fs::read_link("/proc/thread-self/ns/mnt").ok().as_ref() != Some(&self.owned) {
            return Err(hint("namespace", "Cleanup must run in the job's owning namespace."));
        }
        for pass in 0..3 {
            let mut handles = Vec::new();
            for entry in std::fs::read_dir("/proc").map_err(|e| hint("namespace", e.to_string()))? {
                let entry = entry.map_err(|e| hint("namespace", e.to_string()))?;
                let Some(number) =
                    entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok())
                else {
                    continue;
                };
                if number == std::process::id() {
                    continue;
                }
                let Some(pid) = rustix::process::Pid::from_raw(number as i32) else { continue };
                let Ok(fd) = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty())
                else {
                    continue;
                };
                if std::fs::read_link(entry.path().join("ns/mnt")).ok().as_ref()
                    == Some(&self.owned)
                {
                    handles.push(fd);
                }
            }
            if handles.is_empty() {
                return Ok(());
            }
            let signal = if pass == 0 {
                rustix::process::Signal::TERM
            } else {
                rustix::process::Signal::KILL
            };
            for fd in handles {
                let _ = rustix::process::pidfd_send_signal(&fd, signal);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        Ok(())
    }
}

pub(crate) fn mountpoints(text: &str, root: &Path) -> Vec<std::path::PathBuf> {
    let mut paths = text
        .lines()
        .filter_map(|line| line.split_whitespace().nth(4))
        .map(|field| {
            let decoded = field
                .replace("\\040", " ")
                .replace("\\011", "\t")
                .replace("\\012", "\n")
                .replace("\\134", "\\");
            std::path::PathBuf::from(decoded)
        })
        .filter(|path| path.starts_with(root))
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    paths
}

#[cfg(test)]
mod mountinfo_tests {
    use super::*;
    #[test]
    fn native_failure_messages_offer_actionable_recovery() {
        assert!(
            fault_suggestion("apt-get failed", &["E: Unable to locate package missing".into()])
                .contains("catalog bug")
        );
        assert!(
            fault_suggestion(
                "pacman failed",
                &["error: failed retrieving file: Could not resolve host".into()]
            )
            .contains("internet")
        );
        assert!(
            fault_suggestion("dnf failed", &["No space left on device".into()])
                .contains("Free space")
        );
    }
    #[test]
    fn private_bind_children_are_unmounted_without_touching_similar_prefixes() {
        let info = "1 0 0:1 / /run/borea/target rw - btrfs /dev/x rw\n2 1 0:2 / /run/borea/target/dev rw - devtmpfs dev rw\n3 2 0:3 / /run/borea/target/dev/pts rw - devpts pts rw\n4 0 0:4 / /run/borea/target-other rw - ext4 other rw\n5 1 0:5 / /run/borea/target/a\\040b rw - tmpfs tmpfs rw\n";
        let paths = mountpoints(info, Path::new("/run/borea/target"));
        assert_eq!(paths.first().and_then(|path| path.to_str()), Some("/run/borea/target/dev/pts"));
        assert_eq!(paths.last().and_then(|path| path.to_str()), Some("/run/borea/target"));
        assert!(paths.contains(&std::path::PathBuf::from("/run/borea/target/a b")));
        assert_eq!(paths.len(), 4);
    }
    #[test]
    #[allow(clippy::unwrap_used)]
    fn efi_backup_preserves_binary_files_and_rejects_symlinks_and_oversize() {
        let root = std::env::temp_dir().join(format!("borea-efi-fixture-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("nested")).unwrap();
        let data = [0, 255, 13, 10];
        std::fs::write(root.join("nested/BOOTX64.EFI"), data).unwrap();
        let saved = capture_efi(&root).unwrap();
        assert_eq!(saved.get(Path::new("nested/BOOTX64.EFI")), Some(&data.to_vec()));
        std::os::unix::fs::symlink("/etc/passwd", root.join("link")).unwrap();
        assert!(capture_efi(&root).is_err());
        std::fs::remove_file(root.join("link")).unwrap();
        File::create(root.join("large")).unwrap().set_len(129 * 1024 * 1024).unwrap();
        assert!(capture_efi(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
