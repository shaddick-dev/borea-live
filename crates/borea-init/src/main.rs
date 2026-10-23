//! `borea-init`: the first program Borea Live runs, as `/init` in its early boot.
//!
//! 1. Mount `/proc`, `/sys`, and `/dev`.
//! 2. Look at every block device until one carries this build's ID. USB sticks can
//!    appear late, so keep looking for up to 30 seconds.
//! 3. Signed builds: check the system image against the hash in the early boot,
//!    so trust runs from the firmware through Limine to every file.
//! 4. Mount the EROFS system image read-only, straight from the file (no loop
//!    device), with a writable layer in memory over it.
//! 5. Keep the medium mounted at `/media/borea-live` so Borea never offers it as a
//!    target, then switch to the new root and start systemd.
//!
//! On failure it explains what went wrong. Unsigned builds then open busybox's
//! shell for rescue; signed builds never do, and power off instead.

use std::fs;
use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use rustix::mount::{MountFlags, UnmountFlags, mount, mount_move, unmount};
use rustix::process::chroot;
use rustix::system::{RebootCommand, reboot};
use sha2::{Digest, Sha256};

const MEDIUM: &str = "/medium";
const SYSTEM: &str = "/system";
const WRITABLE: &str = "/rw";
const ROOT: &str = "/root";
const IMAGE: &str = "/medium/live/rootfs.erofs";
/// Present only in signed builds: the system image's SHA-256.
const EXPECTED_HASH: &str = "/rootfs.sha256";
const KEEP_MEDIUM_AT: &str = "/root/media/borea-live";
const SYSTEMD: &str = "/usr/lib/systemd/systemd";
const RESCUE_SHELL: &str = "/bin/busybox";

/// How long to wait for the medium, in half-second tries.
const TRIES: u32 = 60;

fn main() {
    let signed = Path::new(EXPECTED_HASH).exists();
    if let Err(problem) = boot(signed) {
        rescue(&problem, signed);
    }
}

fn boot(signed: bool) -> Result<(), String> {
    for (fs_type, target) in [("proc", "/proc"), ("sysfs", "/sys"), ("devtmpfs", "/dev")] {
        fs::create_dir_all(target).ok();
        mount(fs_type, target, fs_type, MountFlags::empty(), None)
            .map_err(|e| format!("could not mount {target}: {e}"))?;
    }
    for dir in [MEDIUM, SYSTEM, WRITABLE, ROOT] {
        fs::create_dir_all(dir).map_err(|e| format!("could not create {dir}: {e}"))?;
    }

    let want =
        fs::read_to_string("/build-id").map_err(|e| format!("the build ID is missing: {e}"))?;
    let device = find_medium(want.trim()).ok_or("its USB stick or disc was not found")?;
    say(&format!("Borea Live: starting from {device}"));

    if signed {
        say("Checking Borea Live's system image…");
        let expected = fs::read_to_string(EXPECTED_HASH).map_err(|e| e.to_string())?;
        let actual = sha256(Path::new(IMAGE))
            .map_err(|e| format!("could not read the system image: {e}"))?;
        if actual != expected.trim() {
            return Err(
                "its system image does not match this build; the medium may be damaged or altered"
                    .into(),
            );
        }
    }

    // EROFS mounts straight from the image file (Linux 6.12 and later).
    mount(IMAGE, SYSTEM, "erofs", MountFlags::RDONLY, None)
        .map_err(|e| format!("the system image would not mount: {e}"))?;
    mount("tmpfs", WRITABLE, "tmpfs", MountFlags::empty(), Some(c"mode=0755"))
        .map_err(|e| format!("no memory for the writable layer: {e}"))?;
    let (upper, work) = (format!("{WRITABLE}/upper"), format!("{WRITABLE}/work"));
    fs::create_dir_all(&upper)
        .and_then(|()| fs::create_dir_all(&work))
        .map_err(|e| e.to_string())?;
    let options = overlay_options(SYSTEM, &upper, &work);
    mount("overlay", ROOT, "overlay", MountFlags::empty(), Some(options.as_c_str()))
        .map_err(|e| format!("the writable layer could not be set up: {e}"))?;

    fs::create_dir_all(KEEP_MEDIUM_AT).map_err(|e| e.to_string())?;
    mount_move(MEDIUM, KEEP_MEDIUM_AT).map_err(|e| format!("could not keep the medium: {e}"))?;
    unmount("/proc", UnmountFlags::DETACH).ok();
    unmount("/sys", UnmountFlags::DETACH).ok();
    mount_move("/dev", "/root/dev").map_err(|e| format!("could not hand over /dev: {e}"))?;

    switch_root()
}

/// Try every block device, newest first to appear last, until one is this build's.
fn find_medium(want: &str) -> Option<String> {
    for _ in 0..TRIES {
        let mut names: Vec<String> = fs::read_dir("/sys/class/block")
            .map(|dir| {
                dir.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        for name in names {
            let device = format!("/dev/{name}");
            if mount(device.as_str(), MEDIUM, "iso9660", MountFlags::RDONLY, None).is_err() {
                continue;
            }
            let found = fs::read_to_string(format!("{MEDIUM}/live/build-id")).unwrap_or_default();
            if is_this_build(&found, want) {
                return Some(device);
            }
            unmount(MEDIUM, UnmountFlags::empty()).ok();
        }
        sleep(Duration::from_millis(500));
    }
    None
}

fn is_this_build(found: &str, want: &str) -> bool {
    !want.is_empty() && found.trim() == want
}

fn overlay_options(lower: &str, upper: &str, work: &str) -> std::ffi::CString {
    std::ffi::CString::new(format!("lowerdir={lower},upperdir={upper},workdir={work}"))
        .expect("paths are fixed and contain no NUL")
}

fn sha256(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 4 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Make the overlay the root, the way `switch_root` does, and start systemd.
fn switch_root() -> Result<(), String> {
    std::env::set_current_dir(ROOT).map_err(|e| e.to_string())?;
    mount_move(".", "/").map_err(|e| format!("could not switch root: {e}"))?;
    chroot(".").map_err(|e| format!("could not switch root: {e}"))?;
    std::env::set_current_dir("/").map_err(|e| e.to_string())?;
    let error = Command::new(SYSTEMD).exec();
    Err(format!("systemd would not start: {error}"))
}

fn say(line: &str) {
    println!("{line}");
}

fn rescue(problem: &str, signed: bool) -> ! {
    say(&format!("\nBorea Live could not start: {problem}"));
    if signed {
        // A signed build never opens a shell that would bypass its own checks.
        say("This is a Secure Boot build. Turning off in 30 seconds.");
        sleep(Duration::from_secs(30));
        let _ = reboot(RebootCommand::PowerOff);
    } else if Path::new(RESCUE_SHELL).exists() {
        say("This is a rescue shell. Reboot to try again.");
        let error = Command::new(RESCUE_SHELL).arg("sh").exec();
        say(&format!("The rescue shell would not start: {error}"));
    }
    loop {
        sleep(Duration::from_secs(3600));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_this_builds_medium_matches() {
        assert!(is_this_build("4f3a9c\n", "4f3a9c"));
        assert!(!is_this_build("4f3a9d\n", "4f3a9c"), "another Borea stick");
        assert!(!is_this_build("", ""), "an empty ID matches nothing");
    }

    #[test]
    fn overlay_layers() {
        let options = overlay_options("/system", "/rw/upper", "/rw/work");
        assert_eq!(
            options.to_str().unwrap(),
            "lowerdir=/system,upperdir=/rw/upper,workdir=/rw/work"
        );
    }

    #[test]
    fn hashes_files_like_sha256sum() {
        let path = std::env::temp_dir().join(format!("borea-init-{}", std::process::id()));
        fs::write(&path, b"borea").unwrap();
        // printf borea | sha256sum
        assert_eq!(
            sha256(&path).unwrap(),
            "1649643f95334e4639d39fbc2e5d6c1a15120e81bb8a06c4006678f9f34ac03d"
        );
        fs::remove_file(path).unwrap();
    }
}
