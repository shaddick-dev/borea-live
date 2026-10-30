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
    let packages = |args: &[String]| {
        !args.is_empty()
            && args.len() <= 512
            && args.iter().all(|s| borea_backend::external::identifier(s))
    };
    if action.program == "/usr/bin/pacman" {
        return action.args.starts_with(&[
            "-Syu".into(),
            "--needed".into(),
            "--noconfirm".into(),
            "--".into(),
        ]) && packages(&action.args[4..]);
    }
    if action.program == "/usr/bin/systemctl" {
        return action.args == ["enable", "--now", "cups.service"];
    }
    action.program == "/usr/bin/flatpak"
        && ((action.args.starts_with(&[
            "install".into(),
            "--system".into(),
            "--noninteractive".into(),
            "-y".into(),
            "flathub".into(),
        ]) && packages(&action.args[5..]))
            || action.args
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
        app_status(path, "installing", queue.len(), None)?;
        println!("Borea: finishing selected apps ({} setup steps remaining).", queue.len());
        if let Err(error) = run(action) {
            app_status(path, "retrying", queue.len(), Some(&error.message))?;
            return Err(error);
        }
        queue.remove(0);
        let bytes = serde_json::to_vec(&queue).map_err(|e| err(e.to_string()))?;
        persist(path, &bytes, 0o600)?;
    }
    println!("Borea: selected application setup is complete.");
    app_status(path, "complete", 0, None)?;
    Ok(())
}

fn app_status(
    queue: &Path,
    state: &str,
    remaining: usize,
    error: Option<&str>,
) -> Result<(), Hint> {
    let path =
        queue.parent().ok_or_else(|| err("No app state directory."))?.join("apps-status.json");
    let status = serde_json::json!({"state":state,"remaining":remaining,"error":error});
    persist(&path, status.to_string().as_bytes(), 0o644)
}

pub fn show_status() -> Result<(), Hint> {
    let path = Path::new("/var/lib/borea/apps-status.json");
    if !path.exists() {
        println!("No Borea app setup has started.");
        return Ok(());
    }
    trusted(path)?;
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(path).map_err(|e| err(e.to_string()))?)
            .map_err(|_| err("Cannot read application setup status."))?;
    println!(
        "Borea apps: {} ({} setup steps remaining).",
        value["state"].as_str().unwrap_or("unknown"),
        value["remaining"].as_u64().unwrap_or(0)
    );
    if let Some(error) = value["error"].as_str() {
        println!("{error}\nDetails: journalctl -u borea-firstboot.service");
    }
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
    run_queue(path, |action| {
        if action.program == "/usr/bin/pacman" && Path::new("/var/lib/pacman/db.lck").exists() {
            return Err(err("Another package transaction is running; Borea will retry later."));
        }
        command(&action.program, &action.args)
    })?;
    if Path::new("/usr/bin/fwupdmgr").is_file() {
        let _ = command("/usr/bin/fwupdmgr", &["refresh".into()]);
    }
    command("/usr/bin/systemctl", &["disable".into(), "borea-firstboot.service".into()])?;
    fs::remove_file(path).map_err(|e| err(e.to_string()))?;
    fs::remove_file("/etc/systemd/system/borea-firstboot.service")
        .map_err(|e| err(e.to_string()))?;
    let dropin = Path::new("/etc/systemd/system/borea-firstboot.service.d/login.conf");
    if dropin.exists() {
        fs::remove_file(dropin).map_err(|e| err(e.to_string()))?;
    }
    command("/usr/bin/systemctl", &["daemon-reload".into()])?;
    Ok(())
}

fn graphical_session(text: &str, user: &str) -> bool {
    let values = text
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<std::collections::BTreeMap<_, _>>();
    values.get("Name") == Some(&user)
        && values.get("Type") == Some(&"wayland")
        && values.get("Class") == Some(&"user")
        && values.get("State") == Some(&"active")
}

/// A root service waits for logind to confirm this account has logged in.
/// No user-writable marker, passwordless sudo, or desktop prompt is required.
pub fn after_login(user: &str) -> Result<(), Hint> {
    if !borea_core::valid_username(user) {
        return Err(err("Invalid first-login account."));
    }
    let queue = Path::new("/var/lib/borea/firstboot.json");
    trusted(queue)?;
    let marker = Path::new("/var/lib/borea/first-login-seen");
    let seen = marker.exists()
        && trusted(marker).is_ok()
        && fs::read_to_string(marker).ok().as_deref() == Some(user);
    if !seen {
        let waiting: Vec<Deferred> =
            serde_json::from_slice(&fs::read(queue).map_err(|e| err(e.to_string()))?)
                .map_err(|_| err("The first-boot queue is invalid."))?;
        app_status(queue, "waiting-for-login", waiting.len(), None)?;
        loop {
            let sessions = Command::new("/usr/bin/loginctl")
                .args(["list-sessions", "--no-legend", "--no-pager"])
                .env("LC_ALL", "C")
                .output()
                .map_err(|_| err("Cannot read login sessions."))?;
            if !sessions.status.success() {
                return Err(err("The login service is not ready; retrying later."));
            }
            let mut logged_in = false;
            for line in String::from_utf8_lossy(&sessions.stdout).lines() {
                let fields = line.split_whitespace().collect::<Vec<_>>();
                if fields.get(2) != Some(&user) {
                    continue;
                }
                let Some(id) = fields.first().filter(|s| borea_backend::external::identifier(s))
                else {
                    continue;
                };
                let session = Command::new("/usr/bin/loginctl")
                    .args([
                        "show-session",
                        id,
                        "--property=Name",
                        "--property=Type",
                        "--property=Class",
                        "--property=State",
                    ])
                    .env("LC_ALL", "C")
                    .output()
                    .map_err(|_| err("Cannot inspect the login session."))?;
                if session.status.success()
                    && graphical_session(&String::from_utf8_lossy(&session.stdout), user)
                {
                    logged_in = true;
                    break;
                }
            }
            if logged_in {
                persist(marker, user.as_bytes(), 0o600)?;
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
    firstboot()
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
    fn native_packages_are_checkpointed_before_a_failed_service_setup() {
        let queue = Queue::new();
        fs::write(
            &queue.0,
            serde_json::json!([
                {"program":"/usr/bin/pacman","args":["-Syu","--needed","--noconfirm","--","cups"]},
                {"program":"/usr/bin/systemctl","args":["enable","--now","cups.service"]}
            ])
            .to_string(),
        )
        .unwrap();
        let mut calls = Vec::new();
        assert!(
            run_queue(&queue.0, |action| {
                calls.push(action.program.clone());
                if action.program.ends_with("systemctl") {
                    Err(err("service failed"))
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
        assert_eq!(calls, ["/usr/bin/pacman", "/usr/bin/systemctl"]);
        calls.clear();
        run_queue(&queue.0, |action| {
            calls.push(action.program.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, ["/usr/bin/systemctl"]);
        let status: serde_json::Value = serde_json::from_slice(
            &fs::read(queue.0.parent().unwrap().join("apps-status.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(status["state"], "complete");
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

#[cfg(test)]
mod login_tests {
    use super::*;
    #[test]
    fn only_the_chosen_users_active_graphical_login_starts_apps() {
        let good = "Name=alice\nType=wayland\nClass=user\nState=active\n";
        assert!(graphical_session(good, "alice"));
        assert!(!graphical_session(good, "bob"));
        assert!(!graphical_session(&good.replace("wayland", "tty"), "alice"));
        assert!(!graphical_session(&good.replace("Class=user", "Class=greeter"), "alice"));
        assert!(!graphical_session(&good.replace("active", "closing"), "alice"));
    }
    #[test]
    fn native_queue_cannot_turn_package_names_into_options_or_run_arbitrary_commands() {
        let make = |args: &[&str]| Deferred {
            program: "/usr/bin/pacman".into(),
            args: args.iter().map(|s| (*s).into()).collect(),
        };
        assert!(allowed(&make(&["-Syu", "--needed", "--noconfirm", "--", "firefox", "steam"])));
        for args in [
            vec!["-Syu", "--needed", "--noconfirm", "--", "--root=/tmp"],
            vec!["-Syu", "--needed", "--noconfirm", "--"],
            vec!["-S", "--noconfirm", "firefox"],
        ] {
            assert!(!allowed(&make(&args)));
        }
        assert!(!allowed(&Deferred {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "id".into()]
        }));
    }
}
