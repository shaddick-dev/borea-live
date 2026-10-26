//! Target helpers must not depend on the live system's libc version.
use borea_core::{Hint, hint};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
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
    fn private_bind_children_are_unmounted_without_touching_similar_prefixes() {
        let info = "1 0 0:1 / /run/borea/target rw - btrfs /dev/x rw\n2 1 0:2 / /run/borea/target/dev rw - devtmpfs dev rw\n3 2 0:3 / /run/borea/target/dev/pts rw - devpts pts rw\n4 0 0:4 / /run/borea/target-other rw - ext4 other rw\n5 1 0:5 / /run/borea/target/a\\040b rw - tmpfs tmpfs rw\n";
        let paths = mountpoints(info, Path::new("/run/borea/target"));
        assert_eq!(paths.first().and_then(|path| path.to_str()), Some("/run/borea/target/dev/pts"));
        assert_eq!(paths.last().and_then(|path| path.to_str()), Some("/run/borea/target"));
        assert!(paths.contains(&std::path::PathBuf::from("/run/borea/target/a b")));
        assert_eq!(paths.len(), 4);
    }
}
