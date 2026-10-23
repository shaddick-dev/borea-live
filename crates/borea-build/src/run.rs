//! Running external tools (make, xorriso, sbsign, …) and hashing files.

use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use blake2::Blake2b512;
use sha2::{Digest, Sha256};

use crate::Result;

/// Extra tool directories (borrowed from Nix), searched before the inherited PATH.
static EXTRA_PATH: OnceLock<Vec<PathBuf>> = OnceLock::new();

/// Put these directories first on the PATH of every program this build starts.
pub fn prefer(dirs: Vec<PathBuf>) {
    let _ = EXTRA_PATH.set(dirs);
}

/// The PATH handed to every program this build starts.
pub fn path() -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let dirs =
        EXTRA_PATH.get().into_iter().flatten().cloned().chain(std::env::split_paths(&inherited));
    std::env::join_paths(dirs).unwrap_or(inherited)
}

/// A command for `program`, found and run with [`path`].
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command.env("PATH", path());
    command
}

/// Run a tool, failing with its name and status if it does not succeed.
pub fn tool(program: &str, args: &[&str]) -> Result {
    let status = command(program)
        .args(args)
        .status()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    status.success().then_some(()).ok_or_else(|| format!("{program} failed ({status})"))
}

/// A path as an argument. Borea's paths are UTF-8; anything else is refused.
pub fn arg(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| format!("{} is not valid UTF-8", path.display()))
}

/// Run a tool quietly and return its standard output.
pub fn output(program: &str, args: &[&str]) -> Result<String> {
    let out = command(program)
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{program} failed ({})", out.status));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("{program} printed invalid UTF-8: {e}"))
}

pub fn on_path(program: &str) -> bool {
    std::env::split_paths(&path()).any(|dir| dir.join(program).is_file())
}

fn digest<D: Digest>(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = D::new();
    let mut buf = vec![0u8; 4 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn sha256(path: &Path) -> Result<String> {
    digest::<Sha256>(path)
}

/// BLAKE2b-512, the hash Limine checks under Secure Boot (as `b2sum` prints it).
pub fn blake2b(path: &Path) -> Result<String> {
    digest::<Blake2b512>(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_match_the_coreutils_tools() {
        let path = std::env::temp_dir().join(format!("borea-build-hash-{}", std::process::id()));
        std::fs::write(&path, b"borea").unwrap();
        // printf borea | sha256sum ; printf borea | b2sum
        assert_eq!(
            sha256(&path).unwrap(),
            output("sha256sum", &[path.to_str().unwrap()]).unwrap()[..64]
        );
        assert_eq!(
            blake2b(&path).unwrap(),
            output("b2sum", &[path.to_str().unwrap()]).unwrap()[..128]
        );
        std::fs::remove_file(path).unwrap();
    }
}
