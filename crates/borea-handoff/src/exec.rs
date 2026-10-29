//! Carry out an approved hand-off. Runs as root inside `borea-installd`.
//!
//! Nothing here touches the target disk except the reviewed [`Change`]s, which
//! are the same `ntfsresize` and `sfdisk` steps a native alongside install runs.
//! Everything else happens on the Borea stick's own data partition.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use borea_core::{Hint, hint};
use borea_disk::Change;
use sha2::{Digest, Sha256};

use crate::boot::{self, IsoFile};
use crate::plan::Review;
use crate::upstream::{self, Upstream};

/// Where Borea Live keeps its own medium mounted (borea-init).
pub const MEDIUM: &str = "/media/borea-live";
/// Where the data partition and the installer ISO are mounted for a hand-off.
pub const DATA: &str = "/run/borea/data";
pub const ISO: &str = "/run/borea/handoff-iso";
/// The directory on the data partition that holds downloaded installers.
pub const DIR: &str = "borea";
const LABELS: [&str; 2] = ["BOREA_DATA", "cidata"];

/// Progress, for the same event stream an install uses.
pub enum Note<'a> {
    Stage(&'a str),
    Line(String),
    Fraction(f32),
}

fn run(program: &str, args: &[&str]) -> Result<String, Hint> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| hint("handoff", format!("{program}: {e}")))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(hint(
            "handoff",
            format!("{program} {} failed: {}", args.join(" "), err.trim()),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn io(context: &str) -> impl Fn(std::io::Error) -> Hint + '_ {
    move |e| hint("handoff", format!("{context}: {e}"))
}

/// The Borea stick's data partition: partition 4 of the disk Borea Live runs from.
pub fn data_partition() -> Result<PathBuf, Hint> {
    let source = run("/usr/bin/findmnt", &["--noheadings", "--output", "SOURCE", MEDIUM])?;
    let name = Path::new(&source)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| hint("handoff", "Borea Live's medium could not be found."))?
        .to_owned();
    let class = Path::new("/sys/class/block").join(&name);
    let disk = if class.join("partition").exists() {
        fs::canonicalize(&class)
            .ok()
            .and_then(|p| {
                p.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned())
            })
            .ok_or_else(|| hint("handoff", "Borea Live's medium could not be found."))?
    } else {
        name
    };
    if disk.starts_with("sr") {
        return Err(hint(
            "handoff",
            "Borea Live is running from a disc. Write it to a USB stick with borea-rataTUI to hand off to other installers.",
        ));
    }
    let p = if disk.ends_with(|c: char| c.is_ascii_digit()) { "p" } else { "" };
    let data = PathBuf::from(format!("/dev/{disk}{p}4"));
    let label = run(
        "/usr/sbin/blkid",
        &["--probe", "--match-tag", "LABEL", "--output", "value", &data.to_string_lossy()],
    )
    .unwrap_or_default();
    if !LABELS.contains(&label.as_str()) {
        return Err(hint(
            "handoff",
            "This Borea stick has no space for other installers. Write it again with the latest borea-rataTUI, on a stick at least 8 GB larger than Borea Live.",
        ));
    }
    Ok(data)
}

fn download(
    agent: &ureq::Agent,
    url: &str,
    to: &Path,
    note: &mut dyn FnMut(Note<'_>),
) -> Result<(), Hint> {
    let part = to.with_extension("part");
    let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut request = agent.get(url);
    if have > 0 {
        request = request.header("Range", format!("bytes={have}-"));
    }
    let mut response = request.call().map_err(|e| hint("download", format!("{url}: {e}")))?;
    let resumed = response.status() == 206;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map(|len| if resumed { len + have } else { len });
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part)
        .map_err(io("the download"))?;
    let mut done = if resumed { have } else { 0 };
    let mut reader = response.body_mut().as_reader();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buffer).map_err(io("the download"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n]).map_err(io("the stick"))?;
        done += n as u64;
        if let Some(total) = total.filter(|t| *t > 0) {
            note(Note::Fraction(done as f32 / total as f32));
        }
    }
    file.sync_all().map_err(io("the stick"))?;
    fs::rename(&part, to).map_err(io("the stick"))
}

fn sha256(path: &Path) -> Result<String, Hint> {
    let mut file = File::open(path).map_err(io("the ISO"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 4 << 20];
    loop {
        let n = file.read(&mut buffer).map_err(io("the ISO"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// `gpgv` must report a valid signature by the pinned key, and nothing else will do.
pub fn signed_by(status: &str, fingerprint: &str) -> bool {
    status.lines().any(|line| {
        let mut words = line.split_whitespace();
        words.next() == Some("[GNUPG:]")
            && words.next() == Some("VALIDSIG")
            && line.split_whitespace().any(|w| w.eq_ignore_ascii_case(fingerprint))
    })
}

fn verify(up: &Upstream, iso: &Path, sums: &Path, sig: &Path) -> Result<(), Hint> {
    let published = fs::read_to_string(sums).map_err(io("the checksum file"))?;
    let want = published.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    if want.len() != 64 || sha256(iso)? != want {
        let _ = fs::remove_file(iso);
        return Err(hint(
            "download",
            format!(
                "The {} ISO doesn't match its published SHA-256; it was deleted. Try again.",
                up.name
            ),
        ));
    }
    let out = Command::new("/usr/bin/gpgv")
        .args(["--status-fd", "1", "--keyring", up.keyring])
        .arg(sig)
        .arg(iso)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| hint("download", format!("gpgv: {e}")))?;
    if !out.status.success() || !signed_by(&String::from_utf8_lossy(&out.stdout), up.fingerprint) {
        return Err(hint(
            "download",
            format!(
                "The {} ISO is not signed by {}'s key {}; it will not be used.",
                up.name, up.name, up.fingerprint
            ),
        ));
    }
    Ok(())
}

fn make_space(disk: &Path, changes: &[Change], sector: u64) -> Result<(), Hint> {
    let disk = disk.to_string_lossy().into_owned();
    for change in changes {
        let Change::ResizeNtfs { path, new_size, number, .. } = change else {
            return Err(hint("disk", "A hand-off only shrinks Windows."));
        };
        let path = path.to_string_lossy().into_owned();
        run(
            "/usr/sbin/ntfsresize",
            &["--no-progress-bar", "--force", "--size", &new_size.to_string(), &path],
        )?;
        let mut sfdisk = Command::new("/usr/sbin/sfdisk")
            .args([
                "--no-reread",
                "--no-tell-kernel",
                "--wipe=never",
                "--wipe-partitions=never",
                "-N",
            ])
            .arg(number.to_string())
            .arg(&disk)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| hint("disk", format!("sfdisk: {e}")))?;
        if let Some(mut input) = sfdisk.stdin.take() {
            writeln!(input, "size={}", new_size / sector).map_err(io("sfdisk"))?;
        }
        let out = sfdisk.wait_with_output().map_err(io("sfdisk"))?;
        if !out.status.success() {
            return Err(hint(
                "disk",
                format!(
                    "Resizing the Windows partition entry failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            ));
        }
    }
    run("/usr/sbin/partprobe", &[&disk])?;
    run("/usr/bin/udevadm", &["settle"])?;
    Ok(())
}

/// Download, verify, prepare the disk and load the installer for `kexec`. The
/// caller has re-reviewed the plan and matched its approval. Returns once the
/// installer is loaded; `systemctl kexec` then starts it.
pub fn prepare(review: &Review, note: &mut dyn FnMut(Note<'_>)) -> Result<(), Hint> {
    let up =
        upstream::find(&review.target).ok_or_else(|| hint("target", "Unknown hand-off target."))?;

    note(Note::Stage("Finding space on the Borea stick."));
    let data = data_partition()?;
    let data_str = data.to_string_lossy().into_owned();
    fs::create_dir_all(DATA).map_err(io(DATA))?;
    if run("/usr/bin/findmnt", &["--noheadings", "--output", "TARGET", DATA]).is_err() {
        // Through a loop device: Borea Live holds its whole stick mounted, and
        // the kernel refuses a second, exclusive claim on one of its partitions.
        run(
            "/usr/bin/mount",
            &["-t", "ext4", "-o", "loop,rw,nosuid,nodev,noexec", &data_str, DATA],
        )?;
    }
    let dir = Path::new(DATA).join(DIR);
    fs::create_dir_all(&dir).map_err(io("the stick"))?;
    let iso = dir.join(up.iso);
    let sums = dir.join(format!("{}.sha256", up.iso));
    let sig = dir.join(format!("{}.sig", up.iso));

    note(Note::Stage(&format!("Downloading {} {}.", up.name, up.version)));
    let agent = borea_http::agent(Duration::from_secs(60))?;
    download(&agent, &up.sha256_url(), &sums, &mut |_| {})?;
    download(&agent, &up.sig_url(), &sig, &mut |_| {})?;
    let cached = iso.exists() && verify(up, &iso, &sums, &sig).is_ok();
    if cached {
        note(Note::Line(format!(
            "{} {} is already on the stick and checks out.",
            up.name, up.version
        )));
    } else {
        download(&agent, &up.iso_url(), &iso, note)?;
        note(Note::Stage(&format!("Checking {} against its signature.", up.name)));
        verify(up, &iso, &sums, &sig)?;
    }

    if !review.changes.is_empty() {
        note(Note::Stage("Making space beside Windows."));
        make_space(&review.disk_path, &review.changes, review.sector)?;
    }

    note(Note::Stage(&format!("Starting {}'s installer.", up.name)));
    fs::create_dir_all(ISO).map_err(io(ISO))?;
    let iso_str = iso.to_string_lossy().into_owned();
    if run("/usr/bin/findmnt", &["--noheadings", "--output", "TARGET", ISO]).is_err() {
        run("/usr/bin/mount", &["-o", "loop,ro", &iso_str, ISO])?;
    }
    let result = load(up, &data_str);
    let _ = run("/usr/bin/umount", &[ISO]);
    result?;
    let _ = run("/usr/bin/sync", &[]);
    Ok(())
}

/// The VM test boots Borea Live with `borea.handoff-console=ttyS0,115200` so it
/// can watch the installer start on the serial console. Only Borea Live's own
/// boot configuration sets this; a normal stick never does.
pub fn test_console(cmdline: &str) -> Option<String> {
    let value =
        cmdline.split_whitespace().find_map(|a| a.strip_prefix("borea.handoff-console="))?;
    value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ',')
        .then(|| format!("console={value} loglevel=6 systemd.show_status=1"))
}

fn load(up: &Upstream, data: &str) -> Result<(), Hint> {
    let config = fs::read_to_string(Path::new(ISO).join(up.boot_config))
        .map_err(io("the installer's boot configuration"))?;
    let uuid = run("/usr/sbin/blkid", &["--match-tag", "UUID", "--output", "value", data])?;
    let path = format!("/{DIR}/{}", up.iso);
    let mut spec =
        boot::boot_spec(&config, up.boot_entry, &IsoFile { uuid: &uuid, path: &path })
            .map_err(|e| hint("handoff", format!("{}'s ISO can't be started: {e}", up.name)))?;
    if let Some(extra) = test_console(&fs::read_to_string("/proc/cmdline").unwrap_or_default()) {
        spec.cmdline.push(' ');
        spec.cmdline.push_str(&extra);
    }
    // kexec takes one initramfs; several concatenate into one, as the kernel allows.
    let initrd = Path::new("/run/borea/handoff-initrd.img");
    let mut joined = File::create(initrd).map_err(io("the initramfs"))?;
    for part in &spec.initrds {
        let mut file = File::open(Path::new(ISO).join(part.trim_start_matches('/')))
            .map_err(io("the installer's initramfs"))?;
        std::io::copy(&mut file, &mut joined).map_err(io("the initramfs"))?;
    }
    joined.sync_all().map_err(io("the initramfs"))?;
    let kernel = Path::new(ISO).join(spec.kernel.trim_start_matches('/'));
    run(
        "/usr/sbin/kexec",
        &[
            "--kexec-file-syscall",
            "--load",
            &kernel.to_string_lossy(),
            &format!("--initrd={}", initrd.display()),
            &format!("--command-line={}", spec.cmdline),
        ],
    )?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_test_console_is_passed_on() {
        assert_eq!(
            test_console("quiet borea.handoff-console=ttyS0,115200").as_deref(),
            Some("console=ttyS0,115200 loglevel=6 systemd.show_status=1")
        );
        assert_eq!(test_console("quiet splash"), None);
        assert_eq!(test_console("borea.handoff-console=ttyS0;init=/bin/sh"), None);
    }

    #[test]
    fn only_a_valid_signature_by_the_pinned_key_counts() {
        let fpr = upstream::OMARCHY.fingerprint;
        let good = format!(
            "[GNUPG:] NEWSIG\n[GNUPG:] GOODSIG F0134EE680CAC571 Omarchy <pkgs@omarchy.org>\n[GNUPG:] VALIDSIG {fpr} 2026-10-20 1792000000 0 4 0 22 10 00 {fpr}\n"
        );
        assert!(signed_by(&good, fpr));
        assert!(!signed_by(&good.replace("VALIDSIG", "BADSIG"), fpr));
        assert!(!signed_by(&good, "0000000000000000000000000000000000000000"));
        assert!(!signed_by("", fpr));
    }
}
