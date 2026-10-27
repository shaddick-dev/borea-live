//! Synthetic Windows-style storage, exclusively inside the marked disposable VM.
//! This checks preservation and resizing; it is not a licensed Windows boot test.
use borea_core::{DiskId, DiskPlan, GIB, Hint, PartId, hint};
use borea_disk::{Disk, DiskFacts, partition_path};
use std::{fs, path::Path, process::Command};
const CONTENT: &[u8] = b"BOREA disposable preservation fixture\n";
fn error(s: impl Into<String>) -> Hint {
    hint("vm-fixture", s)
}
fn output(program: &str, args: &[String]) -> Result<Vec<u8>, Hint> {
    let result = Command::new(borea_platform::live_program(program)?)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| error(e.to_string()))?;
    if !result.status.success() {
        return Err(error(format!(
            "Fixture command {program} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )));
    }
    Ok(result.stdout)
}
fn run(program: &str, args: &[&str]) -> Result<(), Hint> {
    output(program, &args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())?;
    Ok(())
}
pub fn prepare(disk: &Disk) -> Result<(), Hint> {
    if disk.id.serial.as_deref() != Some("BOREA-TEST-TARGET")
        || disk.live
        || disk.in_use
        || disk.readonly
        || disk.id.size < 90 * GIB
        || fs::read_to_string("/sys/firmware/qemu_fw_cfg/by_name/opt/borea/autotest/raw")
            .ok()
            .as_deref()
            != Some("BOREA_DISPOSABLE_VM")
    {
        return Err(error("Refusing to construct a fixture outside its dedicated VM disk."));
    }
    let device = disk.id.path.to_string_lossy();
    run("/usr/sbin/sgdisk", &["--zap-all", &device])?;
    run(
        "/usr/sbin/sgdisk",
        &[
            "--new=1:2048:+100M",
            "--typecode=1:ef00",
            "--new=2:0:+16M",
            "--typecode=2:0c01",
            "--new=3:0:0",
            "--typecode=3:0700",
            &device,
        ],
    )?;
    run("/usr/sbin/partprobe", &[&device])?;
    run("/usr/bin/udevadm", &["settle"])?;
    let esp = partition_path(&disk.id.path, 1);
    let windows = partition_path(&disk.id.path, 3);
    run("/usr/sbin/mkfs.fat", &["-F", "32", &esp.to_string_lossy()])?;
    run(
        "/usr/sbin/mkfs.ntfs",
        &["--fast", "--force", "--label", "BOREA_WINDOWS", &windows.to_string_lossy()],
    )?;
    fs::write("/run/borea-preserve-fixture", CONTENT).map_err(|e| error(e.to_string()))?;
    run(
        "/usr/sbin/ntfscp",
        &[&windows.to_string_lossy(), "/run/borea-preserve-fixture", "/BOREA-PRESERVE.txt"],
    )?;
    run(
        "/usr/bin/mmd",
        &[
            "-i",
            &esp.to_string_lossy(),
            "::EFI",
            "::EFI/Microsoft",
            "::EFI/Microsoft/Boot",
            "::EFI/BOOT",
        ],
    )?;
    for path in ["::EFI/Microsoft/Boot/bootmgfw.efi", "::EFI/BOOT/BOOTX64.EFI"] {
        run(
            "/usr/bin/mcopy",
            &["-i", &esp.to_string_lossy(), "/run/borea-preserve-fixture", path],
        )?;
    }
    run("/usr/bin/udevadm", &["trigger", "--action=change"])?;
    run("/usr/bin/udevadm", &["settle"])?;
    Ok(())
}
pub struct Preserved {
    disk: DiskId,
    uuid: String,
    original_size: u64,
}
impl Preserved {
    pub fn select(disk: &Disk) -> Result<(Self, DiskPlan), Hint> {
        let windows = disk
            .parts
            .iter()
            .find(|p| p.number == 3 && p.fs == "ntfs")
            .ok_or_else(|| error("The synthetic Windows NTFS partition was not discovered."))?;
        let fixture =
            Self { disk: disk.id.clone(), uuid: windows.uuid.clone(), original_size: windows.size };
        let plan = DiskPlan::Alongside {
            disk: disk.id.clone(),
            windows_part: PartId {
                path: windows.path.clone(),
                uuid: windows.uuid.clone(),
                number: 3,
            },
            linux_size: 40 * GIB,
        };
        Ok((fixture, plan))
    }
    pub fn verify(&self, facts: &DiskFacts) -> Result<(), Hint> {
        let disk = facts
            .disks
            .iter()
            .find(|d| d.id == self.disk)
            .ok_or_else(|| error("The fixture disk changed identity."))?;
        let windows = disk
            .parts
            .iter()
            .find(|p| p.number == 3 && p.uuid == self.uuid && p.fs == "ntfs")
            .ok_or_else(|| error("The original NTFS partition was lost."))?;
        if windows.size >= self.original_size
            || windows.size < self.original_size - 40 * GIB - 1024 * 1024
        {
            return Err(error("The NTFS partition was not resized to the approved geometry."));
        }
        if output(
            "/usr/sbin/ntfscat",
            // ntfsresize schedules chkdsk. This known fixture must remain
            // readable for verification; ntfscat still opens it read-only.
            &["--force".into(), windows.path.display().to_string(), "/BOREA-PRESERVE.txt".into()],
        )? != CONTENT
        {
            return Err(error("NTFS contents changed during installation."));
        }
        let esp = partition_path(Path::new(&self.disk.path), 1);
        for path in ["::EFI/Microsoft/Boot/bootmgfw.efi", "::EFI/BOOT/BOOTX64.EFI"] {
            if output("/usr/bin/mtype", &["-i".into(), esp.display().to_string(), path.into()])?
                != CONTENT
            {
                return Err(error("Existing EFI loader contents changed during installation."));
            }
        }
        println!("BOREA_PRESERVATION_OK");
        Ok(())
    }
}
