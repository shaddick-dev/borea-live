//! No compilation or source substitution when the latest stable v5 is unavailable.
use borea_core::{Hint, hint};
use serde_json::Value;

fn metadata(url: &str) -> Result<Value, Hint> {
    borea_http::agent(std::time::Duration::from_secs(60))?
        .get(url)
        .header("User-Agent", "Borea/0.1")
        .header("Accept", "application/json")
        .call()
        .map_err(|error| {
            let reason = match error {
                ureq::Error::StatusCode(code) => format!("HTTP {code}"),
                ureq::Error::Timeout(_) => "request timed out".into(),
                ureq::Error::Rustls(_) | ureq::Error::Tls(_) => "TLS verification failed".into(),
                ureq::Error::ConnectProxyFailed(_) => "proxy connection failed".into(),
                _ => "connection failed".into(),
            };
            hint(
                "desktop",
                format!(
                    "Cannot check Noctalia releases ({reason}). Check the connection and retry."
                ),
            )
        })?
        .body_mut()
        .read_to_string()
        .map_err(|_| hint("desktop", "Cannot read Noctalia release metadata."))
        .and_then(|text| {
            serde_json::from_str(&text)
                .map_err(|_| hint("desktop", "Invalid Noctalia release metadata."))
        })
}

pub fn latest_v5(releases: &Value) -> Result<String, Hint> {
    releases
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["draft"] == false && r["prerelease"] == false)
        .filter_map(|r| r["tag_name"].as_str())
        .filter_map(|tag| {
            let version = tag.strip_prefix('v').unwrap_or(tag);
            let numbers =
                version.split('.').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>().ok()?;
            (numbers.len() == 3 && numbers[0] == 5).then(|| (numbers, version.to_owned()))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, version)| version)
        .ok_or_else(|| hint("desktop", "Upstream lists no stable Noctalia v5 release."))
}

/// Accept Arch's epoch/pkgrel suffix, but require the exact approved upstream version.
pub fn check_version(package: &str, approved: &str) -> Result<(), Hint> {
    let version = package.split_once(':').map_or(package, |(_, v)| v);
    let version = version.rsplit_once('-').map_or(version, |(v, _)| v);
    if !approved.starts_with("5.") || version != approved {
        return Err(hint(
            "desktop",
            format!(
                "Noctalia {approved} is required, but Arch offers {package}. Wait for the signed Arch package to update, then review again. Nothing has been written to the target disk."
            ),
        ));
    }
    Ok(())
}

pub fn resolve() -> Result<String, Hint> {
    let latest = metadata("https://api.github.com/repos/noctalia-dev/noctalia/releases/latest")?;
    let version = match latest_v5(&serde_json::json!([latest])) {
        Ok(version) => version,
        Err(_) => latest_v5(&metadata(
            "https://api.github.com/repos/noctalia-dev/noctalia/releases?per_page=100",
        )?)?,
    };
    let index = metadata("https://archlinux.org/packages/search/json/?name=noctalia")?;
    let package = index["results"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["pkgname"] == "noctalia" && p["repo"] == "extra" && p["arch"] == "x86_64")
        .and_then(|p| p["pkgver"].as_str())
        .ok_or_else(|| hint("desktop", "Arch extra has no x86-64 Noctalia package."))?;
    check_version(package, &version)?;
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn stable_major_five_only_and_numeric_sort() {
        let releases = json!([
            {"tag_name":"v6.0.0","draft":false,"prerelease":false},
            {"tag_name":"v5.10.1","draft":false,"prerelease":false},
            {"tag_name":"v5.9.9","draft":false,"prerelease":false},
            {"tag_name":"v5.11.0-rc1","draft":false,"prerelease":true},
            {"tag_name":"v5.12.0","draft":true,"prerelease":false}
        ]);
        assert_eq!(latest_v5(&releases).ok().as_deref(), Some("5.10.1"));
        assert!(check_version("5.4.0-1", "5.4.1").is_err());
        assert!(check_version("1:5.4.1-2", "5.4.1").is_ok());
        assert!(check_version("5.4.1", "5.4.1").is_ok());
        assert!(check_version("6.0.0-1", "5.4.1").is_err());
    }
}
