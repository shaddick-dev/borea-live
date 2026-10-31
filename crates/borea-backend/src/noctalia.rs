//! Stable Noctalia v5 from the selected distro's signed native repositories.
use borea_core::{Hint, hint};

/// Review approves this stable major; pacman selects its newest native package.
pub const REQUIREMENT: &str = "5";
const MINIMUM: [u32; 3] = [5, 4, 0];

/// Reject development builds, another major, and releases older than the config API.
/// Older jobs carrying an exact reviewed version retain their exact comparison.
pub fn check_version(package: &str, approved: &str) -> Result<(), Hint> {
    let (epoch, version) = package.split_once(':').map_or((None, package), |(e, v)| (Some(e), v));
    let (version, release) =
        version.rsplit_once('-').map_or((version, None), |(v, r)| (v, Some(r)));
    let numeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let native_stable =
        epoch.is_none_or(numeric) && release.is_none_or(|r| r.split('.').all(numeric));
    let numbers = version.split('.').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>();
    let supported = numbers.is_ok_and(|v| {
        v.len() == 3
            && v[0] == 5
            && v.as_slice() >= MINIMUM.as_slice()
            && version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    });
    if !native_stable || !supported || (approved != REQUIREMENT && version != approved) {
        return Err(hint(
            "desktop",
            format!(
                "A stable Noctalia v5 package (5.4.0 or newer) is required; the selected repository offers {package}. Review the installation again after its signed package is updated. Nothing has been written to the target disk."
            ),
        ));
    }
    Ok(())
}

pub fn resolve() -> Result<String, Hint> {
    Ok(REQUIREMENT.into())
}

pub fn resolve_for(_cachyos: bool) -> Result<String, Hint> {
    resolve()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_signed_repository_packages_do_not_need_github_patch_parity() {
        for package in ["5.4.0-1", "5.4.1-1", "1:5.4.1-2", "5.10.0-1"] {
            assert!(check_version(package, REQUIREMENT).is_ok(), "{package}");
        }
        for package in [
            "4.9.9-1",
            "6.0.0-1",
            "5.3.9-1",
            "5.4.1-rc1-1",
            "5.4.1.r3-1",
            "5.4.1+git-1",
            "5.4.1.1-1",
            "garbage",
            "5.4.1-rc1",
            "x:5.4.0-1",
        ] {
            assert!(check_version(package, REQUIREMENT).is_err(), "{package}");
        }
    }
    #[test]
    fn previously_approved_exact_versions_remain_exact() {
        assert!(check_version("5.4.0-1", "5.4.1").is_err());
        assert!(check_version("1:5.4.1-2", "5.4.1").is_ok());
        assert!(check_version("5.4.1", "5.4.1").is_ok());
        assert!(check_version("6.0.0-1", "5.4.1").is_err());
    }
    #[test]
    fn both_native_targets_approve_stable_major_without_web_metadata() {
        assert_eq!(resolve().ok().as_deref(), Some(REQUIREMENT));
        for cachyos in [false, true] {
            assert_eq!(resolve_for(cachyos).ok().as_deref(), Some(REQUIREMENT));
        }
    }
}
