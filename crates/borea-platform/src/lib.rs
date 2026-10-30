#![forbid(unsafe_code)]
//! Read-only Linux runtime differences; never searches a caller-controlled PATH.
use borea_core::{Hint, hint};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Component, Path},
};
pub mod firmware;
pub mod power;
pub fn program(root: &Path, canonical: &str) -> Result<String, Hint> {
    let path = Path::new(canonical);
    if !path.is_absolute()
        || path.components().any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(hint("runtime", "A native executable path is invalid."));
    }
    let exists = |candidate: &str| {
        root.join(candidate.trim_start_matches('/'))
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    };
    if exists(canonical) {
        return Ok(canonical.into());
    }
    let parent = path.parent().and_then(Path::to_str);
    if matches!(parent, Some("/usr/bin" | "/usr/sbin" | "/bin" | "/sbin")) {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| hint("runtime", "The native executable name is invalid."))?;
        for directory in ["/usr/bin", "/usr/sbin", "/bin", "/sbin"] {
            let candidate = format!("{directory}/{name}");
            if exists(&candidate) {
                return Ok(candidate);
            }
        }
    }
    Err(hint("runtime", format!("The environment is missing executable {canonical}.")))
}
pub fn live_program(canonical: &str) -> Result<String, Hint> {
    program(Path::new("/"), canonical)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merged_and_separate_usr_layouts_are_supported_without_path_search() {
        let root = std::env::temp_dir().join(format!("borea-tool-path-{}", std::process::id()));
        assert!(std::fs::create_dir_all(root.join("usr/bin")).is_ok());
        let executable = root.join("usr/bin/btrfs");
        assert!(std::fs::write(&executable, b"fixture").is_ok());
        assert!(
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).is_ok()
        );
        assert_eq!(program(&root, "/usr/sbin/btrfs").ok().as_deref(), Some("/usr/bin/btrfs"));
        assert!(program(&root, "/usr/sbin/../bin/btrfs").is_err());
        assert!(program(&root, "/opt/btrfs").is_err());
        assert!(
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o644)).is_ok()
        );
        assert!(program(&root, "/usr/bin/btrfs").is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
