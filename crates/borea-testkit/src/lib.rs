//! Test fixtures shared across Borea crates.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A throwaway directory tree standing in for `/`, removed on drop.
///
/// Probes take a root path, so a fixture can mimic any machine: write the procfs and
/// sysfs files a scenario needs and point the probe at [`FakeRoot::path`].
pub struct FakeRoot(PathBuf);

impl FakeRoot {
    pub fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("borea-root-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create fixture root");
        FakeRoot(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Write a file, creating its parents. Returns `self` so scenarios read as a list.
    pub fn file(&self, rel: &str, contents: impl AsRef<[u8]>) -> &Self {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().expect("relative path has a parent"))
            .expect("create fixture dirs");
        fs::write(path, contents).expect("write fixture file");
        self
    }

    /// Create an empty directory, e.g. a sysfs marker such as `wireless/`.
    pub fn dir(&self, rel: &str) -> &Self {
        fs::create_dir_all(self.0.join(rel)).expect("create fixture dir");
        self
    }

    /// Create a symlink, e.g. a sysfs `driver` link.
    pub fn link(&self, rel: &str, target: &str) -> &Self {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().expect("relative path has a parent"))
            .expect("create fixture dirs");
        std::os::unix::fs::symlink(self.0.join(target), path).expect("create fixture link");
        self
    }
}

impl Default for FakeRoot {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FakeRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
