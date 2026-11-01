//! The silent worker gives way to the person using the system. It never starts
//! a package transaction while theirs is running, holds pacman's or zypper's
//! lock only for the install itself, and waits for a lock rather than failing.
use std::fs;
use std::path::Path;

/// Package managers and AUR helpers a person may be running.
const MANAGERS: &[&str] = &[
    "pacman", "paru", "yay", "pamac", "makepkg", "pacstrap", "zypper", "dnf", "dnf5", "apt",
    "apt-get", "dpkg", "aptitude", "snapper",
];

/// A package manager running now, other than Borea's own children, if any.
pub fn running_in(proc: &Path, ours: u32) -> Option<String> {
    fs::read_dir(proc).into_iter().flatten().flatten().find_map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        let pid = name.parse::<u32>().ok()?;
        if pid == ours || parent(&entry.path()) == Some(ours) {
            return None;
        }
        let comm = fs::read_to_string(entry.path().join("comm")).ok()?;
        MANAGERS.contains(&comm.trim()).then(|| comm.trim().to_owned())
    })
}
fn parent(process: &Path) -> Option<u32> {
    let stat = fs::read_to_string(process.join("stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}
/// Whether zypper's lock (/run/zypp.pid) is held by a live process: PackageKit
/// or a person's zypper.
pub fn zypp_locked_in(lock: &Path, proc: &Path) -> bool {
    fs::read_to_string(lock)
        .ok()
        .and_then(|pid| pid.trim().parse::<u32>().ok())
        .is_some_and(|pid| pid > 0 && proc.join(pid.to_string()).exists())
}

/// Wait, taking no lock, until no one else is changing packages.
pub fn wait_for_quiet() {
    let ours = std::process::id();
    let mut told = false;
    loop {
        let busy = running_in(Path::new("/proc"), ours).or_else(|| {
            zypp_locked_in(Path::new("/run/zypp.pid"), Path::new("/proc"))
                .then(|| "PackageKit or zypper".to_owned())
        });
        let Some(who) = busy else { return };
        if !told {
            println!("Borea: waiting for {who} to finish before continuing.");
            told = true;
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

/// zypper waits for its lock instead of failing (Borea's own: indefinitely).
pub const ZYPP_WAIT: &str = "ZYPP_LOCK_TIMEOUT";

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    fn process(proc: &Path, pid: u32, comm: &str, ppid: u32) {
        let dir = proc.join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("comm"), format!("{comm}\n")).unwrap();
        fs::write(dir.join("stat"), format!("{pid} ({comm}) S {ppid} 1 1")).unwrap();
    }
    #[test]
    fn others_package_managers_are_waited_for_and_borea_s_own_are_not() {
        let proc = std::env::temp_dir().join(format!("borea-courtesy-{}", std::process::id()));
        process(&proc, 10, "bash", 1);
        process(&proc, 20, "borea", 1);
        assert_eq!(running_in(&proc, 20), None);
        process(&proc, 21, "pacman", 20);
        assert_eq!(running_in(&proc, 20), None, "Borea's own pacman");
        process(&proc, 30, "paru", 10);
        assert_eq!(running_in(&proc, 20).as_deref(), Some("paru"));
        fs::remove_dir_all(proc.join("30")).unwrap();
        process(&proc, 31, "zypper", 10);
        assert_eq!(running_in(&proc, 20).as_deref(), Some("zypper"));
        // zypper's lock: held by a live process, or left stale.
        let lock = proc.join("zypp.pid");
        fs::write(&lock, "31\n").unwrap();
        assert!(zypp_locked_in(&lock, &proc));
        fs::write(&lock, "999\n").unwrap();
        assert!(!zypp_locked_in(&lock, &proc));
        fs::remove_dir_all(proc).unwrap();
    }
}
