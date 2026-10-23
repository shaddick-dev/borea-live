//! Downloads and their checks: HTTPS with bundled root certificates, SHA-256 on
//! every byte, and OpenPGP signatures against keys pinned in this source.

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::Path;

use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};
use pgp::types::KeyDetails;
use sha2::{Digest, Sha256};

use crate::plan::hex;

/// The largest text file read into memory (checksum lists, signatures).
const TEXT_LIMIT: u64 = 1 << 20;

pub fn text(url: &str) -> Result<String, String> {
    let mut body = String::new();
    open(url)?.take(TEXT_LIMIT).read_to_string(&mut body).map_err(|e| format!("{url}: {e}"))?;
    Ok(body)
}

pub fn bytes(url: &str) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    open(url)?.take(TEXT_LIMIT).read_to_end(&mut body).map_err(|e| format!("{url}: {e}"))?;
    Ok(body)
}

fn open(url: &str) -> Result<impl Read, String> {
    let response = ureq::get(url).call().map_err(|e| format!("{url}: {e}"))?;
    Ok(response.into_body().into_reader())
}

/// Download `url` to `dest` (through a temporary name), refusing more than
/// `limit` bytes. Returns the SHA-256 and size of what was written.
pub fn download(
    url: &str,
    dest: &Path,
    limit: u64,
    mut progress: impl FnMut(u64),
) -> Result<(String, u64), String> {
    let partial = dest.with_extension("part");
    let mut out = File::create(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
    let mut body = open(url)?.take(limit + 1);
    let (sha256, size) =
        copy_hashing(&mut body, &mut out, &mut progress).map_err(|e| format!("{url}: {e}"))?;
    if size > limit {
        let _ = fs::remove_file(&partial);
        return Err(format!("{url} is larger than the {limit} bytes expected"));
    }
    out.sync_all().map_err(|e| e.to_string())?;
    fs::rename(&partial, dest).map_err(|e| e.to_string())?;
    Ok((sha256, size))
}

/// Copy everything from `from` to `to`, hashing on the way.
pub fn copy_hashing(
    from: &mut impl Read,
    to: &mut impl Write,
    progress: &mut impl FnMut(u64),
) -> io::Result<(String, u64)> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    let mut total = 0;
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        hasher.update(&buf[..n]);
        to.write_all(&buf[..n])?;
        total += n as u64;
        progress(total);
    }
    Ok((hex(&hasher.finalize()), total))
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    copy_hashing(&mut file, &mut io::sink(), &mut |_| {}).map(|(sha, _)| sha)
}

/// Check a detached signature over `file` against the armored public `key`.
/// Returns the signing key's fingerprint, upper-case hex.
pub fn verify_signature(key: &str, signature: &[u8], file: &Path) -> Result<String, String> {
    let (key, _) = SignedPublicKey::from_armor_single(key.as_bytes())
        .map_err(|e| format!("pinned key: {e}"))?;
    key.verify_bindings().map_err(|e| format!("pinned key: {e}"))?;
    let signature =
        DetachedSignature::from_bytes(signature).map_err(|e| format!("signature: {e}"))?.signature;
    let data = File::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    signature
        .verify(&key, BufReader::new(data))
        .map_err(|e| format!("{}: bad signature: {e}", file.display()))?;
    Ok(format!("{:X}", key.fingerprint()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_what_it_copies() {
        let mut out = Vec::new();
        let mut seen = 0;
        let (sha, size) = copy_hashing(&mut &b"abc"[..], &mut out, &mut |n| seen = n).unwrap();
        assert_eq!(sha, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!((size, seen, out.as_slice()), (3, 3, &b"abc"[..]));
    }

    #[test]
    fn the_pinned_arch_key_is_the_one_archlinux_org_lists() {
        let (key, _) = SignedPublicKey::from_armor_single(crate::arch::RELEASE_KEY.as_bytes())
            .expect("parses");
        assert_eq!(format!("{:X}", key.fingerprint()), crate::arch::RELEASE_KEY_FINGERPRINT);
        key.verify_bindings().expect("self-signatures hold");
    }

    #[test]
    fn a_tampered_file_fails_its_signature() {
        let dir = std::env::temp_dir().join(format!("borea-sig-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("data");
        fs::write(&file, "not what was signed").unwrap();
        // A real signature by the Arch key, over a different file.
        let signature = include_bytes!("../tests/bootstrap.sig");
        let result = verify_signature(crate::arch::RELEASE_KEY, signature, &file);
        assert!(result.unwrap_err().contains("bad signature"));
        fs::remove_dir_all(dir).unwrap();
    }
}
