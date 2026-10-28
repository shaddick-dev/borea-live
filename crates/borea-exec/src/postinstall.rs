//! The target's small first-boot/updater helper. Its root-owned queues contain no secrets.
use borea_core::{Hint, hint};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    fs::OpenOptions,
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    process::Command,
};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageRecord {
    pub image: borea_backend::external::AppImage,
    pub user: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deferred {
    pub program: String,
    pub args: Vec<String>,
}
fn err(s: impl Into<String>) -> Hint {
    hint("firstboot", s)
}
fn trusted(path: &Path) -> Result<(), Hint> {
    let m = fs::symlink_metadata(path).map_err(|e| err(e.to_string()))?;
    if !m.is_file() || m.uid() != 0 || m.permissions().mode() & 0o022 != 0 {
        return Err(err(
            "The first-boot configuration must be a root-owned, non-writable regular file.",
        ));
    }
    Ok(())
}
fn persist(path: &Path, bytes: &[u8], mode: u32) -> Result<(), Hint> {
    let temporary = path.with_extension(format!("{}.pending", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(mode)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&temporary)
        .map_err(|e| err(e.to_string()))?;
    let result = (|| {
        file.write_all(bytes).map_err(|e| err(e.to_string()))?;
        file.sync_all().map_err(|e| err(e.to_string()))?;
        fs::rename(&temporary, path).map_err(|e| err(e.to_string()))?;
        fs::File::open(path.parent().ok_or_else(|| err("Missing configuration directory."))?)
            .and_then(|directory| directory.sync_all())
            .map_err(|e| err(e.to_string()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
fn command(program: &str, args: &[String]) -> Result<(), Hint> {
    let status = Command::new(program)
        .args(args)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .status()
        .map_err(|e| err(e.to_string()))?;
    if !status.success() {
        return Err(err(format!("{program} failed.")));
    }
    Ok(())
}

fn allowed(action: &Deferred) -> bool {
    action.program == "/usr/bin/flatpak"
        && (action.args.starts_with(&[
            "install".into(),
            "--system".into(),
            "--noninteractive".into(),
            "-y".into(),
            "flathub".into(),
        ]) || action.args
            == [
                "remote-add",
                "--system",
                "--noninteractive",
                "--if-not-exists",
                "flathub",
                "https://dl.flathub.org/repo/flathub.flatpakrepo",
            ])
}

/// Save each successful step before continuing; a later failure retains only the
/// remaining work. Flatpak's noninteractive operations are safe to repeat if power
/// is lost after an install but before its checkpoint is committed.
fn run_queue(path: &Path, mut run: impl FnMut(&Deferred) -> Result<(), Hint>) -> Result<(), Hint> {
    let mut queue: Vec<Deferred> =
        serde_json::from_slice(&fs::read(path).map_err(|e| err(e.to_string()))?)
            .map_err(|_| err("The first-boot queue is invalid."))?;
    if !queue.iter().all(allowed) {
        return Err(err("The first-boot queue contains an unexpected command."));
    }
    while let Some(action) = queue.first() {
        println!("Borea: finishing selected apps ({} setup steps remaining).", queue.len());
        run(action)?;
        queue.remove(0);
        let bytes = serde_json::to_vec(&queue).map_err(|e| err(e.to_string()))?;
        persist(path, &bytes, 0o600)?;
    }
    println!("Borea: selected application setup is complete.");
    Ok(())
}
/// This entry runs in its own helper process as the target user, never as root.
pub fn extract_image(directory: &str, filename: &str) -> Result<(), Hint> {
    if nix::unistd::Uid::effective().is_root() {
        return Err(err("AppImages must be extracted by an unprivileged account."));
    }
    if !Path::new(directory).is_absolute() || !borea_backend::external::identifier(filename) {
        return Err(err("The AppImage extraction path is invalid."));
    }
    nix::sys::stat::umask(nix::sys::stat::Mode::from_bits_truncate(0o022));
    let status = Command::new(Path::new(directory).join(filename))
        .arg("--appimage-extract")
        .current_dir(directory)
        .status()
        .map_err(|e| err(e.to_string()))?;
    if status.success() { Ok(()) } else { Err(err("The AppImage could not be extracted.")) }
}
pub fn firstboot() -> Result<(), Hint> {
    let path = Path::new("/var/lib/borea/firstboot.json");
    trusted(path)?;
    run_queue(path, |action| command(&action.program, &action.args))?;
    if Path::new("/usr/bin/fwupdmgr").is_file() {
        let _ = command("/usr/bin/fwupdmgr", &["refresh".into()]);
    }
    command("/usr/bin/systemctl", &["disable".into(), "borea-firstboot.service".into()])?;
    fs::remove_file(path).map_err(|e| err(e.to_string()))?;
    fs::remove_file("/etc/systemd/system/borea-firstboot.service")
        .map_err(|e| err(e.to_string()))?;
    command("/usr/bin/systemctl", &["daemon-reload".into()])?;
    Ok(())
}
pub fn update_images() -> Result<(), Hint> {
    let mut failures = Vec::new();
    for entry in fs::read_dir("/etc/borea/appimages").map_err(|e| err(e.to_string()))? {
        let path = entry.map_err(|e| err(e.to_string()))?.path();
        if path.extension().is_none_or(|s| s != "json") {
            continue;
        }
        if let Err(e) = update_one(&path) {
            failures.push(e.message);
        }
    }
    if failures.is_empty() { Ok(()) } else { Err(err(failures.join("; "))) }
}
fn update_one(config: &Path) -> Result<(), Hint> {
    trusted(config)?;
    let old: ImageRecord =
        serde_json::from_slice(&fs::read(config).map_err(|e| err(e.to_string()))?)
            .map_err(|_| err("An AppImage update record is invalid."))?;
    if !borea_core::valid_username(&old.user) {
        return Err(err("The AppImage owner is invalid."));
    }
    let next = borea_backend::external::appimage(&old.image.repository)?;
    if next.sha256 == old.image.sha256 {
        return Ok(());
    }
    if old.image.published_checksum && !next.published_checksum {
        return Err(err(
            "The new release lost its published checksum; automatic update was refused.",
        ));
    }
    let id = old.image.repository.replace('/', "-");
    if !borea_backend::external::identifier(&id) {
        return Err(err("The AppImage identifier is invalid."));
    }
    let parent = Path::new("/opt/borea/appimages");
    let base = parent.join(&id);
    let staging = parent.join(format!("{id}.next"));
    let backup = parent.join(format!("{id}.previous"));
    if staging.exists() || backup.exists() {
        return Err(err("An earlier AppImage update needs cleanup."));
    }
    fs::create_dir(&staging).map_err(|e| err(e.to_string()))?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o755))
        .map_err(|e| err(e.to_string()))?;
    let binary = staging.join(&next.filename);
    crate::download(&next.url, &binary, Some(&next.sha256))?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
        .map_err(|e| err(e.to_string()))?;
    command(
        "/usr/bin/chown",
        &["-R".into(), format!("{}:{}", old.user, old.user), staging.display().to_string()],
    )?;
    command(
        "/usr/sbin/runuser",
        &[
            "-u".into(),
            old.user.clone(),
            "--".into(),
            "/usr/bin/borea-firstboot".into(),
            "--extract-appimage".into(),
            staging.display().to_string(),
            next.filename.clone(),
        ],
    )?;
    let apprun = staging.join("squashfs-root/AppRun");
    if !apprun.exists() {
        return Err(err("The updated AppImage has no AppRun entry point."));
    }
    fs::rename(&base, &backup).map_err(|e| err(e.to_string()))?;
    if let Err(e) = fs::rename(&staging, &base) {
        let _ = fs::rename(&backup, &base);
        return Err(err(e.to_string()));
    }
    let record = ImageRecord { image: next, user: old.user };
    persist(config, &serde_json::to_vec_pretty(&record).map_err(|e| err(e.to_string()))?, 0o644)?;
    fs::remove_dir_all(backup).map_err(|e| err(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Queue(std::path::PathBuf);
    impl Queue {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "borea-firstboot-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&dir).unwrap();
            let path = dir.join("firstboot.json");
            let queue = serde_json::json!([
                {"program": "/usr/bin/flatpak", "args": [
                    "remote-add", "--system", "--noninteractive", "--if-not-exists", "flathub",
                    "https://dl.flathub.org/repo/flathub.flatpakrepo"
                ]},
                {"program": "/usr/bin/flatpak", "args": [
                    "install", "--system", "--noninteractive", "-y", "flathub",
                    "org.mozilla.firefox"
                ]}
            ]);
            fs::write(&path, queue.to_string()).unwrap();
            Self(path)
        }
    }
    impl Drop for Queue {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.0.parent().unwrap());
        }
    }

    #[test]
    fn failed_download_resumes_without_repeating_completed_setup() {
        let queue = Queue::new();
        let mut calls = Vec::new();
        let failed = run_queue(&queue.0, |action| {
            calls.push(action.args[0].clone());
            if action.args[0] == "install" { Err(err("Network disconnected")) } else { Ok(()) }
        });
        assert!(failed.is_err());
        assert_eq!(calls, ["remote-add", "install"]);
        let remaining: Vec<Deferred> =
            serde_json::from_slice(&fs::read(&queue.0).unwrap()).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].args[0], "install");
        assert_eq!(fs::metadata(&queue.0).unwrap().permissions().mode() & 0o777, 0o600);
        calls.clear();
        run_queue(&queue.0, |action| {
            calls.push(action.args[0].clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, ["install"]);
        assert_eq!(fs::read_to_string(&queue.0).unwrap(), "[]");
        run_queue(&queue.0, |_| Err(err("An empty queue must not execute anything"))).unwrap();
    }

    #[test]
    fn invalid_queue_is_rejected_before_any_command_executes() {
        let queue = Queue::new();
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&queue.0).unwrap()).unwrap();
        value[1]["program"] = "/bin/sh".into();
        fs::write(&queue.0, value.to_string()).unwrap();
        let mut executed = false;
        assert!(
            run_queue(&queue.0, |_| {
                executed = true;
                Ok(())
            })
            .is_err()
        );
        assert!(!executed);
        let other_remote = Deferred {
            program: "/usr/bin/flatpak".into(),
            args: vec!["remote-add".into(), "unreviewed".into(), "https://example.org".into()],
        };
        assert!(!allowed(&other_remote));
    }
}
