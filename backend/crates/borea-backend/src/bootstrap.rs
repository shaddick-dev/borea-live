//! Upstream tool-root release resolution. Signature verification occurs before execution.
use crate::{Artifact, Bootstrap, ChecksumProof};
use borea_core::{Distribution as Distro, Hint, Plan, hint};
fn text(url: &str) -> Result<String, Hint> {
    let agent = borea_http::agent(std::time::Duration::from_secs(20))?;
    agent
        .get(url)
        .call()
        .map_err(|_| hint("bootstrap", "The upstream release metadata is not reachable."))?
        .body_mut()
        .read_to_string()
        .map_err(|_| hint("bootstrap", "The upstream release metadata cannot be read."))
}
pub fn checksum_matches(text: &str, name: &str, hash: &str) -> bool {
    find_hash(text, name).is_some_and(|s| s.eq_ignore_ascii_case(hash))
}
fn find_hash(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let parts = l.split_whitespace().collect::<Vec<_>>();
        if parts.len() >= 2
            && parts[0].len() == 64
            && parts[0].bytes().all(|b| b.is_ascii_hexdigit())
            && parts[1].trim_start_matches('*') == name
        {
            return Some(parts[0].to_ascii_lowercase());
        }
        let prefix = format!("SHA256 ({name}) = ");
        l.strip_prefix(&prefix)
            .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_ascii_lowercase)
    })
}
fn suse_archive(listing: &str) -> Result<String, Hint> {
    let mut names = listing
        .split(['\"', '\''])
        .filter(|name| {
            name.starts_with("openSUSE-Tumbleweed-")
                && name.contains("Container")
                && name.contains("Base")
                && name.contains("x86_64")
                && [".tar.xz", ".tar.zst", ".tar.gz"].iter().any(|suffix| name.ends_with(suffix))
                && crate::external::identifier(name)
        })
        .collect::<Vec<_>>();
    names.sort_by_key(|name| (!name.contains("latest"), *name));
    names.dedup();
    names.last().map(|name| (*name).to_owned()).ok_or_else(|| {
        hint(
            "bootstrap",
            "The official Tumbleweed appliance listing contains no x86-64 Container Base archive.",
        )
    })
}
pub fn resolve(plan: &Plan) -> Result<Bootstrap, Hint> {
    let keyring = format!("/usr/share/keyrings/borea/{}.gpg", plan.distro.key());
    let artifact = match plan.distro {
        Distro::Debian => None,
        Distro::Arch => {
            let base = "https://geo.mirror.pkgbuild.com/iso/latest";
            let sums = text(&format!("{base}/sha256sums.txt"))?;
            let (name, sha256) = sums
                .lines()
                .find_map(|l| {
                    let p = l.split_whitespace().collect::<Vec<_>>();
                    if p.len() == 2
                        && p[0].len() == 64
                        && p[1].starts_with("archlinux-bootstrap-")
                        && p[1].ends_with("-x86_64.tar.zst")
                    {
                        Some((p[1].to_owned(), p[0].to_owned()))
                    } else {
                        None
                    }
                })
                .ok_or_else(|| {
                    hint("bootstrap", "The mirror lists no dated Arch bootstrap archive.")
                })?;
            let url = format!("{base}/{name}");
            Some(Artifact {
                checksum: None,
                signature_url: format!("{url}.sig"),
                url,
                sha256,
                keyring,
            })
        }
        Distro::Fedora => {
            let base = format!(
                "https://download.fedoraproject.org/pub/fedora/linux/releases/{}/Container/x86_64/images",
                plan.release
            );
            let listing = text(&format!("{base}/"))?;
            let mut names = listing
                .split('"')
                .filter(|s| {
                    s.starts_with(&format!("Fedora-Container-Base-Generic-{}-", plan.release))
                        && s.ends_with(".x86_64.tar.xz")
                        && !s.contains('/')
                })
                .collect::<Vec<_>>();
            names.sort();
            names.dedup();
            let name = names.last().ok_or_else(|| {
                hint(
                    "bootstrap",
                    "No official Fedora Container Base artifact was listed for this release.",
                )
            })?;
            let checksums = listing
                .split('"')
                .find(|s| {
                    s.starts_with("Fedora-Container-")
                        && s.ends_with("CHECKSUM")
                        && !s.contains('/')
                })
                .ok_or_else(|| {
                    hint("bootstrap", "The Fedora container release has no signed checksum file.")
                })?;
            let metadata = format!("{base}/{checksums}");
            let sums = text(&metadata)?;
            let sha256 = find_hash(&sums, name).ok_or_else(|| {
                hint("bootstrap", "The container image has no published SHA-256.")
            })?;
            Some(Artifact {
                checksum: Some(ChecksumProof { url: metadata.clone(), filename: (*name).into() }),
                signature_url: metadata,
                url: format!("{base}/{name}"),
                sha256,
                keyring,
            })
        }
        Distro::OpenSuse => {
            let base = "https://download.opensuse.org/tumbleweed/appliances";
            let listing = text(&format!("{base}/"))?;
            let name = suse_archive(&listing)?;
            let url = format!("{base}/{name}");
            let metadata = format!("{url}.sha256");
            let sums = text(&metadata)?;
            let sha256 = find_hash(&sums, &name).ok_or_else(|| {
                hint("bootstrap", "The Tumbleweed rootfs has no published SHA-256.")
            })?;
            let signature_url = if sums.starts_with("-----BEGIN PGP SIGNED MESSAGE-----") {
                metadata.clone()
            } else {
                format!("{metadata}.asc")
            };
            Some(Artifact {
                checksum: Some(ChecksumProof { url: metadata, filename: name }),
                signature_url,
                url,
                sha256,
                keyring,
            })
        }
    };
    let result = Bootstrap { distro: plan.distro, release: plan.release.clone(), artifact };
    result.validate()?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checksum_formats() {
        let h = "a".repeat(64);
        assert!(checksum_matches(&format!("{h}  image.tar.xz"), "image.tar.xz", &h));
        assert!(checksum_matches(&format!("SHA256 (image.tar.xz) = {h}"), "image.tar.xz", &h));
        assert!(!checksum_matches(&format!("{h}  other.tar"), "image.tar.xz", &h));
    }
    #[test]
    fn tumbleweed_discovers_dated_native_archives_without_inventing_a_filename() {
        let dated = "openSUSE-Tumbleweed-Container-Base.x86_64-20261024.tar.xz";
        let listing = format!(
            "<a href=\"openSUSE-Tumbleweed-Container-Base.latest.x86_64.tar.xz\">latest</a><a href='{dated}'>dated</a><a href=\"../openSUSE-Tumbleweed-Container-Base.x86_64.tar.xz\">escape</a><a href=\"openSUSE-Tumbleweed-Container-Base.aarch64.tar.xz\">other</a>"
        );
        assert_eq!(suse_archive(&listing).ok().as_deref(), Some(dated));
        assert!(suse_archive("<a href=\"unrelated.tar.xz\">wrong</a>").is_err());
    }
}
