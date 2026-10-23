//! The image being written: what it is, how big, and whether it is the one
//! that was published.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::FlashError;

/// ISO 9660 places its primary volume descriptor at sector 16 (2048-byte sectors).
const PVD: u64 = 16 * 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub path: PathBuf,
    pub size: u64,
    /// ISO 9660 volume label, e.g. `BOREA_LIVE_202610`.
    pub label: Option<String>,
    /// Has an MBR boot signature, so it boots when written raw to a USB stick.
    pub hybrid: bool,
}

impl Image {
    pub fn name(&self) -> String {
        self.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    /// Read the headers and refuse anything that is not a bootable ISO image.
    pub fn inspect(path: &Path) -> Result<Image, FlashError> {
        let mut file =
            File::open(path).map_err(|e| FlashError::Image(path.into(), e.to_string()))?;
        let size =
            file.metadata().map_err(|e| FlashError::Image(path.into(), e.to_string()))?.len();
        let not_iso = || FlashError::Image(path.into(), "not an ISO 9660 image".into());
        if size < PVD + 2048 {
            return Err(not_iso());
        }
        let mut mbr = [0u8; 512];
        let mut pvd = [0u8; 2048];
        let read = |file: &mut File, at: u64, buf: &mut [u8]| -> io::Result<()> {
            file.seek(SeekFrom::Start(at))?;
            file.read_exact(buf)
        };
        read(&mut file, 0, &mut mbr)
            .and_then(|()| read(&mut file, PVD, &mut pvd))
            .map_err(|_| not_iso())?;
        let (kind, magic) = (pvd[0], &pvd[1..6]);
        if kind != 1 || magic != b"CD001" {
            return Err(not_iso());
        }
        let label = String::from_utf8_lossy(&pvd[40..72]).trim().to_owned();
        let image = Image {
            path: path.into(),
            size,
            label: (!label.is_empty()).then_some(label),
            hybrid: mbr[510..512] == [0x55, 0xAA],
        };
        if !image.hybrid {
            return Err(FlashError::Image(
                path.into(),
                "this ISO has no USB boot record (it is not a hybrid image)".into(),
            ));
        }
        Ok(image)
    }
}

/// The published SHA-256 for `image`, from `<image>.sha256` or a `SHA256SUMS` file
/// beside it. Returns the expected digest and the file it came from.
pub fn published_checksum(image: &Path) -> Option<(String, PathBuf)> {
    let name = image.file_name()?.to_string_lossy().into_owned();
    let dir = image.parent().unwrap_or(Path::new("."));
    let candidates =
        [dir.join(format!("{name}.sha256")), dir.join("SHA256SUMS"), dir.join("sha256sums.txt")];
    candidates.into_iter().find_map(|file| {
        let text = std::fs::read_to_string(&file).ok()?;
        let digest = text.lines().find_map(|line| {
            let mut fields = line.split_whitespace();
            let digest = fields.next()?;
            let listed = fields.next().map(|f| f.trim_start_matches('*'));
            let valid = digest.len() == 64 && digest.chars().all(|c| c.is_ascii_hexdigit());
            (valid && listed.is_none_or(|f| f == name)).then(|| digest.to_ascii_lowercase())
        })?;
        Some((digest, file))
    })
}

/// SHA-256 of a file, reporting bytes read as it goes.
pub fn sha256(path: &Path, mut progress: impl FnMut(u64)) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 4 << 20];
    let mut done = 0;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(done);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal hybrid ISO: MBR signature, primary volume descriptor, a label.
    pub fn fake_iso(dir: &Path, name: &str, label: &str, size: usize) -> PathBuf {
        let mut bytes = vec![0u8; size.max(PVD as usize + 4096)];
        bytes[510] = 0x55;
        bytes[511] = 0xAA;
        let pvd = PVD as usize;
        bytes[pvd] = 1;
        bytes[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        let mut padded = [b' '; 32];
        padded[..label.len()].copy_from_slice(label.as_bytes());
        bytes[pvd + 40..pvd + 72].copy_from_slice(&padded);
        for (i, b) in bytes.iter_mut().enumerate().skip(pvd + 2048) {
            *b = (i % 251) as u8;
        }
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    pub fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("borea-flash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_label_and_boot_record() {
        let dir = temp_dir("inspect");
        let path = fake_iso(&dir, "borea-live.iso", "BOREA_LIVE_202610", 1 << 20);
        let image = Image::inspect(&path).unwrap();
        assert_eq!(image.label.as_deref(), Some("BOREA_LIVE_202610"));
        assert!(image.hybrid);
        assert_eq!(image.size, 1 << 20);
    }

    #[test]
    fn refuses_files_that_are_not_bootable_isos() {
        let dir = temp_dir("refuse");
        let text = dir.join("notes.iso");
        std::fs::write(&text, vec![b'x'; 1 << 20]).unwrap();
        assert!(matches!(Image::inspect(&text), Err(FlashError::Image(..))));

        let iso = fake_iso(&dir, "plain.iso", "PLAIN", 1 << 20);
        let mut bytes = std::fs::read(&iso).unwrap();
        bytes[510] = 0;
        std::fs::write(&iso, bytes).unwrap();
        let error = Image::inspect(&iso).unwrap_err().to_string();
        assert!(error.contains("not a hybrid image"), "{error}");
    }

    #[test]
    fn finds_published_checksums() {
        let dir = temp_dir("sums");
        let iso = fake_iso(&dir, "borea-live.iso", "BOREA", 1 << 20);
        assert_eq!(published_checksum(&iso), None);
        let digest = sha256(&iso, |_| {}).unwrap();
        std::fs::write(
            dir.join("SHA256SUMS"),
            format!("{} *other.iso\n{digest}  borea-live.iso\n", "a".repeat(64)),
        )
        .unwrap();
        let (found, from) = published_checksum(&iso).unwrap();
        assert_eq!(found, digest);
        assert!(from.ends_with("SHA256SUMS"));
        std::fs::write(dir.join("borea-live.iso.sha256"), format!("{digest}\n")).unwrap();
        assert!(published_checksum(&iso).unwrap().1.ends_with("borea-live.iso.sha256"));
    }
}
