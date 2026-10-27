//! Diagnose the installed boot path after a failed disposable Debian run.
//! This emits a distinct repair marker and can never count as a full install pass.
use borea_core::{GIB, Hint, hint};
use borea_disk::{Disk, partition_path};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
const ROOT: &str = "/run/borea-test-repair";
fn run(program: &str, args: &[&str]) -> Result<(), Hint> {
    let status = Command::new(borea_platform::live_program(program)?)
        .args(args)
        .env("LC_ALL", "C")
        .status()
        .map_err(|e| hint("vm-repair", e.to_string()))?;
    if !status.success() {
        return Err(hint("vm-repair", format!("{program} failed: {status}")));
    }
    Ok(())
}
pub fn debian_boot(disk: &Disk) -> Result<(), Hint> {
    if disk.id.serial.as_deref() != Some("BOREA-TEST-TARGET")
        || disk.id.size != 40 * GIB
        || disk.parts.len() != 3
        || disk.parts.iter().any(|p| p.number == 3 && p.fs != "crypto_LUKS")
        || disk.in_use
        || disk.live
        || disk.readonly
        || Path::new("/dev/mapper/borea-test-repair").exists()
        || fs::read_to_string("/sys/firmware/qemu_fw_cfg/by_name/opt/borea/autotest/raw")
            .ok()
            .as_deref()
            != Some("BOREA_DISPOSABLE_VM")
    {
        return Err(hint(
            "vm-repair",
            "Repair only accepts the original disposable whole-drive Debian fixture.",
        ));
    }
    let root_part = partition_path(&disk.id.path, 3);
    let mut child = Command::new(borea_platform::live_program("/usr/sbin/cryptsetup")?)
        .args(["open", "--key-file", "-"])
        .arg(&root_part)
        .arg("borea-test-repair")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| hint("vm-repair", e.to_string()))?;
    let sent = child
        .stdin
        .take()
        .ok_or_else(|| hint("vm-repair", "No fixture credential pipe."))?
        .write_all(b"borea-disposable-vm-ONLY");
    if let Err(error) = sent {
        let _ = child.kill();
        let _ = child.wait();
        return Err(hint("vm-repair", error.to_string()));
    }
    if !child.wait().map_err(|e| hint("vm-repair", e.to_string()))?.success() {
        return Err(hint("vm-repair", "Could not unlock the disposable fixture."));
    }
    let result = (|| {
        fs::create_dir(ROOT).map_err(|e| hint("vm-repair", e.to_string()))?;
        run(
            "/usr/bin/mount",
            &["-t", "btrfs", "-o", "subvol=@", "/dev/mapper/borea-test-repair", ROOT],
        )?;
        run(
            "/usr/bin/mount",
            &[&partition_path(&disk.id.path, 2).to_string_lossy(), &format!("{ROOT}/boot")],
        )?;
        run(
            "/usr/bin/mount",
            &[&partition_path(&disk.id.path, 1).to_string_lossy(), &format!("{ROOT}/boot/efi")],
        )?;
        for source in ["/proc", "/sys", "/dev"] {
            run("/usr/bin/mount", &["--rbind", source, &format!("{ROOT}{source}")])?;
            run("/usr/bin/mount", &["--make-rslave", &format!("{ROOT}{source}")])?;
        }
        run("/usr/sbin/chroot", &[ROOT, "/usr/bin/mkdir", "-p", "/boot/grub"])?;
        run("/usr/sbin/chroot", &[ROOT, "/usr/sbin/update-grub"])?;
        run("/usr/bin/sync", &[])?;
        Ok(())
    })();
    let unmount = run("/usr/bin/umount", &["--recursive", ROOT]);
    let close = run("/usr/sbin/cryptsetup", &["close", "borea-test-repair"]);
    result?;
    unmount?;
    close?;
    println!("BOREA_BOOT_REPAIR_COMPLETE");
    Ok(())
}
