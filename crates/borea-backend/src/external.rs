//! Resolve AUR snapshots and upstream AppImages BEFORE approving or executing disk writes.
use borea_core::{Hint, Review, hint};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalSources {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noctalia: Option<String>,
    #[serde(default)]
    pub repository_keys: Vec<RepositoryKey>,
    pub aur: Vec<AurRecipe>,
    pub appimages: Vec<AppImage>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryKey {
    pub source: String,
    pub url: String,
    pub sha256: String,
}
pub fn repository_key(source: &str, _release: &str) -> Result<Option<RepositoryKey>, Hint> {
    let url: String = match source {
        "fedora.rpmfusion-free" => "https://download1.rpmfusion.org/free/fedora/RPM-GPG-KEY-rpmfusion-free-fedora-2020".into(),
        "fedora.rpmfusion-nonfree" => "https://download1.rpmfusion.org/nonfree/fedora/RPM-GPG-KEY-rpmfusion-nonfree-fedora-2020".into(),
        "opensuse.packman" => "https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Tumbleweed/repodata/repomd.xml.key".into(),
        "opensuse.nvidia" => "https://download.nvidia.com/opensuse/tumbleweed/repodata/repomd.xml.key".into(),
        _ => return Ok(None),
    };
    let data = bytes(&url)?;
    if !data.starts_with(b"-----BEGIN PGP PUBLIC KEY BLOCK-----") {
        return Err(hint(
            "sources",
            "The selected repository did not publish an armored signing key.",
        ));
    }
    Ok(Some(RepositoryKey { source: source.into(), url, sha256: digest(&data) }))
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AurRecipe {
    pub base: String,
    pub url: String,
    pub sha256: String,
    pub packages: Vec<String>,
    pub dependencies: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppImage {
    pub repository: String,
    pub tag: String,
    pub url: String,
    pub sha256: String,
    pub published_checksum: bool,
    pub filename: String,
}
pub fn identifier(s: &str) -> bool {
    s.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && s.len() < 128
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._+".contains(&b))
}
fn bytes(url: &str) -> Result<Vec<u8>, Hint> {
    if !url.starts_with("https://") {
        return Err(hint("sources", "Upstream sources require HTTPS."));
    }
    let agent = borea_http::agent(std::time::Duration::from_secs(60))?;
    agent
        .get(url)
        .header("User-Agent", "Borea/0.1")
        .header("Accept", "application/json")
        .call()
        .map_err(|_| hint("sources", "The upstream release metadata is not reachable."))?
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|_| hint("sources", "The upstream metadata could not be read."))
}
fn json(url: &str) -> Result<serde_json::Value, Hint> {
    serde_json::from_slice(&bytes(url)?)
        .map_err(|_| hint("sources", "Upstream returned invalid release metadata."))
}
fn digest(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
pub fn resolve(review: &Review) -> Result<ExternalSources, Hint> {
    let mut result = ExternalSources::default();
    if review.desktop == Some(borea_core::Desktop::HyprlandNoctalia) {
        result.noctalia = Some(crate::noctalia::resolve()?);
    }
    let mut recipes = BTreeMap::new();
    let mut visiting = BTreeSet::new();
    for p in &review.packages {
        if p.source == "aur" {
            aur(&p.name, &mut recipes, &mut visiting, &mut result.aur)?;
        } else if p.source == "appimage" {
            result.appimages.push(appimage(&p.name)?);
        } else if p.source == "flathub" {
            flathub(&p.name)?;
        }
    }
    Ok(result)
}
/// Check the public app metadata before allowing disk writes. Flatpak still
/// authenticates the repository and payload with its native signing policy.
pub fn flathub(id: &str) -> Result<(), Hint> {
    if !identifier(id) || !id.contains('.') {
        return Err(hint("flathub", "The Flathub application identifier is invalid."));
    }
    validate_flathub(id, &json(&format!("https://flathub.org/api/v2/appstream/{id}"))?)
}
fn validate_flathub(id: &str, metadata: &serde_json::Value) -> Result<(), Hint> {
    if metadata["id"].as_str() != Some(id)
        || metadata["is_eol"].as_bool() != Some(false)
        || metadata["bundle"]["type"].as_str() != Some("flatpak")
        || !metadata["bundle"]["value"]
            .as_str()
            .is_some_and(|value| value.starts_with(&format!("app/{id}/x86_64/")))
    {
        return Err(hint(
            "flathub",
            format!("The Flathub app {id} is unavailable or no longer maintained."),
        ));
    }
    Ok(())
}

pub fn verify_source(source: &str, name: &str) -> Result<(), Hint> {
    match source {
        "flathub" => flathub(name),
        "appimage" => appimage(name).map(|_| ()),
        "aur" if identifier(name) => {
            let metadata = json(&format!("https://aur.archlinux.org/rpc/v5/info?arg[]={name}"))?;
            if metadata["results"]
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["Name"].as_str() == Some(name)))
            {
                Ok(())
            } else {
                Err(hint("aur", format!("The AUR package {name} is unavailable.")))
            }
        }
        _ => Err(hint("sources", "Choose a supported external package source and identifier.")),
    }
}
fn aur(
    name: &str,
    done: &mut BTreeMap<String, String>,
    visiting: &mut BTreeSet<String>,
    out: &mut Vec<AurRecipe>,
) -> Result<(), Hint> {
    if visiting.len() > 128 || done.len() > 1024 {
        return Err(hint("aur", "The AUR dependency graph exceeds the installation limit."));
    }
    if !identifier(name) {
        return Err(hint("aur", "An AUR package name is invalid."));
    }
    if done.contains_key(name) {
        return Ok(());
    }
    if !visiting.insert(name.into()) {
        return Err(hint("aur", "The AUR dependencies contain a cycle."));
    }
    let info = json(&format!("https://aur.archlinux.org/rpc/v5/info?arg[]={name}"))?;
    let item = info["results"]
        .as_array()
        .and_then(|r| r.first())
        .ok_or_else(|| hint("aur", format!("The AUR package {name} is no longer available.")))?;
    let base = item["PackageBase"]
        .as_str()
        .filter(|s| identifier(s))
        .ok_or_else(|| hint("aur", "The AUR package base is invalid."))?
        .to_owned();
    if out.iter().any(|r| r.base == base) {
        if let Some(recipe) = out.iter_mut().find(|r| r.base == base) {
            if !recipe.packages.iter().any(|p| p == name) {
                recipe.packages.push(name.into());
            }
        }
        done.insert(name.into(), base);
        visiting.remove(name);
        return Ok(());
    }
    let mut deps = BTreeSet::new();
    for key in ["Depends", "MakeDepends", "CheckDepends"] {
        if let Some(list) = item[key].as_array() {
            for dep in list.iter().filter_map(|v| v.as_str()) {
                let key = dep.split(['<', '>', '=']).next().unwrap_or(dep);
                if !identifier(key) {
                    return Err(hint("aur", "An AUR dependency name is invalid."));
                }
                let lookup = json(&format!("https://aur.archlinux.org/rpc/v5/info?arg[]={key}"))?;
                if lookup["resultcount"].as_u64().unwrap_or(0) > 0 {
                    aur(key, done, visiting, out)?;
                } else {
                    deps.insert(dep.to_owned());
                }
            }
        }
    }
    let url = format!("https://aur.archlinux.org/cgit/aur.git/snapshot/{base}.tar.gz");
    let hash = digest(&bytes(&url)?);
    out.push(AurRecipe {
        base: base.clone(),
        url,
        sha256: hash,
        packages: vec![name.into()],
        dependencies: deps.into_iter().collect(),
    });
    done.insert(name.into(), base);
    visiting.remove(name);
    Ok(())
}
pub fn appimage(repository: &str) -> Result<AppImage, Hint> {
    let split = repository.split('/').collect::<Vec<_>>();
    if split.len() != 2 || split.iter().any(|s| !identifier(s)) {
        return Err(hint("appimage", "The upstream repository is invalid."));
    }
    let release = json(&format!("https://api.github.com/repos/{repository}/releases/latest"))?;
    let tag = release["tag_name"]
        .as_str()
        .ok_or_else(|| hint("appimage", "The upstream release has no version tag."))?
        .to_owned();
    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| hint("appimage", "This release has no published artifacts."))?;
    let choices = assets
        .iter()
        .filter(|a| {
            a["name"].as_str().is_some_and(|n| {
                let n = n.to_ascii_lowercase();
                n.ends_with(".appimage")
                    && !n.contains("aarch64")
                    && !n.contains("arm64")
                    && !n.contains("armv7")
                    && !n.contains("i386")
                    && !n.contains("i686")
            })
        })
        .collect::<Vec<_>>();
    let chosen = if choices.len() == 1 {
        choices[0]
    } else {
        let exact = choices
            .iter()
            .filter(|a| {
                a["name"].as_str().is_some_and(|s| s.contains("x86_64") || s.contains("amd64"))
            })
            .collect::<Vec<_>>();
        if exact.len() != 1 {
            return Err(hint(
                "appimage",
                "The upstream release does not have one unambiguous x86-64 AppImage.",
            ));
        }
        exact[0]
    };
    let url = chosen["browser_download_url"]
        .as_str()
        .filter(|s| s.starts_with(&format!("https://github.com/{repository}/releases/download/")))
        .ok_or_else(|| {
            hint("appimage", "The AppImage asset is not from its upstream GitHub release.")
        })?
        .to_owned();
    let filename = chosen["name"]
        .as_str()
        .ok_or_else(|| hint("appimage", "The AppImage filename is missing."))?
        .to_owned();
    if !identifier(&filename) {
        return Err(hint("appimage", "The AppImage filename is unsafe."));
    }
    let published = chosen["digest"]
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()));
    // Where no published hash exists, pin the bytes for Review and explicitly flag provenance.
    let (sha256, published_checksum) = match published {
        Some(hash) => (hash.to_ascii_lowercase(), true),
        None => (hash_url(&url)?, false),
    };
    Ok(AppImage { repository: repository.into(), tag, url, sha256, published_checksum, filename })
}

fn hash_url(url: &str) -> Result<String, Hint> {
    use std::io::Read;
    let agent = borea_http::agent(std::time::Duration::from_secs(600))?;
    let mut response = agent
        .get(url)
        .header("User-Agent", "Borea/0.1")
        .call()
        .map_err(|_| hint("appimage", "The upstream AppImage is not reachable."))?;
    let mut reader = response.body_mut().as_reader();
    let mut hash = Sha256::new();
    let mut bytes = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let n = reader
            .read(&mut bytes)
            .map_err(|_| hint("appimage", "The upstream AppImage download failed."))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > 4 * 1024 * 1024 * 1024 {
            return Err(hint("appimage", "The AppImage exceeds the download limit."));
        }
        hash.update(&bytes[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    #[test]
    fn flathub_requires_matching_maintained_x86_64_application() {
        let mut data = serde_json::json!({"id":"org.test.App", "is_eol":false, "bundle":{"type":"flatpak","value":"app/org.test.App/x86_64/stable"}});
        assert!(validate_flathub("org.test.App", &data).is_ok());
        assert!(validate_flathub("org.other.App", &data).is_err());
        data["is_eol"] = true.into();
        assert!(validate_flathub("org.test.App", &data).is_err());
        data["is_eol"] = false.into();
        data["bundle"]["value"] = "app/org.test.App/aarch64/stable".into();
        assert!(validate_flathub("org.test.App", &data).is_err());
    }
}
