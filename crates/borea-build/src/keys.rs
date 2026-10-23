//! `borea-build keys DIR`: a Secure Boot signing key for your own Borea Live builds.
//!
//! Writes `db.key` (private), `db.crt` (PEM, for `borea-build iso --secure-boot`),
//! and `db.cer` (DER, to enroll in firmware setup or with a key manager). Machines
//! boot a signed Borea Live only after this certificate is in their db; nothing
//! changes on machines that never enroll it.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::Result;
use crate::run::{arg, output, tool};

pub fn make(dir: &Path) -> Result {
    let key = dir.join("db.key");
    if key.exists() {
        return Err(format!("{} exists; not overwriting it", key.display()));
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let host = output("uname", &["-n"]).unwrap_or_default();
    let subject = format!("/CN=Borea Live Secure Boot ({})/", host.trim());
    let (crt, cer) = (dir.join("db.crt"), dir.join("db.cer"));
    // RSA keys, because firmware Secure Boot databases broadly support only RSA.
    tool(
        "openssl",
        &[
            "req",
            "-new",
            "-x509",
            "-newkey",
            "rsa:3072",
            "-sha256",
            "-days",
            "3650",
            "-nodes",
            "-subj",
            &subject,
            "-keyout",
            arg(&key)?,
            "-out",
            arg(&crt)?,
        ],
    )?;
    tool("openssl", &["x509", "-in", arg(&crt)?, "-outform", "DER", "-out", arg(&cer)?])?;
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    println!("Made {}, db.crt, db.cer", key.display());
    println!("Build signed:  borea-build iso --secure-boot {}", dir.display());
    println!("Then enroll {} in each machine's Secure Boot db.", cer.display());
    println!("Keep db.key private: anything signed with it boots on those machines.");
    Ok(())
}

/// The key belongs to the certificate: checked before hours of building.
pub fn check_pair(key: &Path, cert: &Path) -> Result {
    let from_key = output("openssl", &["pkey", "-in", arg(key)?, "-pubout"])?;
    let from_cert = output("openssl", &["x509", "-in", arg(cert)?, "-noout", "-pubkey"])?;
    (from_key == from_cert)
        .then_some(())
        .ok_or_else(|| format!("{} does not match {}", key.display(), cert.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("borea-keys-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn makes_a_private_matching_pair_and_never_overwrites() {
        let dir = temp("make");
        make(&dir).unwrap();
        let mode = fs::metadata(dir.join("db.key")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        check_pair(&dir.join("db.key"), &dir.join("db.crt")).unwrap();
        assert!(make(&dir).unwrap_err().contains("not overwriting"));

        let other = temp("other");
        make(&other).unwrap();
        assert!(check_pair(&dir.join("db.key"), &other.join("db.crt")).is_err());
        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_dir_all(other);
    }
}
