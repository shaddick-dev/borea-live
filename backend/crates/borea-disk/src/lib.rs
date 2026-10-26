#![forbid(unsafe_code)]
//! Read-only device discovery and pure UEFI partition layout calculation.
mod journal;
mod ntfs;
use borea_core::{DiskId, DiskPlan, Distribution as Distro, GIB, Hint, hint};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Partition {
    pub path: PathBuf,
    pub number: u32,
    pub uuid: String,
    pub start: u64,
    pub size: u64,
    pub fs: String,
    pub efi: bool,
    pub free: Option<u64>,
    pub bitlocker: bool,
    pub ntfs_checked: bool,
    pub ntfs_hibernated: bool,
    pub ntfs_dirty: bool,
    pub ntfs_min_size: Option<u64>,
}
impl Partition {
    /// Space available for Linux after retaining the verified NTFS minimum and
    /// 30 GiB of additional Windows space, rounded down to a MiB boundary.
    pub fn max_linux_size(&self) -> Option<u64> {
        if self.fs != "ntfs"
            || !self.ntfs_checked
            || self.ntfs_dirty
            || self.ntfs_hibernated
            || self.bitlocker
        {
            return None;
        }
        let available = self.size.checked_sub(self.ntfs_min_size?.checked_add(30 * GIB)?)?;
        Some(available / (1024 * 1024) * (1024 * 1024))
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disk {
    pub id: DiskId,
    pub readonly: bool,
    pub in_use: bool,
    pub live: bool,
    pub removable: bool,
    pub transport: String,
    pub parts: Vec<Partition>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskFacts {
    pub uefi: bool,
    pub disks: Vec<Disk>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layout {
    pub disk: DiskId,
    pub changes: Vec<Change>,
    pub esp: PathBuf,
    pub esp_mount: String,
    pub boot: Option<PathBuf>,
    pub boot_fs: String,
    pub esp_number: u32,
    pub esp_uuid: Option<String>,
    pub esp_size: u64,
    pub root: PathBuf,
    pub root_subvol: String,
    pub subvolumes: Vec<(String, String)>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Change {
    Wipe,
    ResizeNtfs {
        path: PathBuf,
        new_size: u64,
        number: u32,
        end_sector: u64,
    },
    Create {
        number: u32,
        start_sector: u64,
        end_sector: u64,
        type_code: String,
        label: String,
        path: PathBuf,
    },
}
fn txt(v: &serde_json::Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().into()
}
fn num(v: &serde_json::Value, k: &str) -> u64 {
    v[k].as_u64().or_else(|| v[k].as_str().and_then(|s| s.parse().ok())).unwrap_or(0)
}
fn flag(v: &serde_json::Value, k: &str) -> bool {
    v[k].as_bool().unwrap_or_else(|| num(v, k) != 0)
}
fn mounted(v: &serde_json::Value) -> bool {
    v["mountpoints"]
        .as_array()
        .is_some_and(|a| a.iter().any(|s| s.as_str().is_some_and(|p| !p.is_empty())))
        || v["children"].as_array().is_some_and(|a| a.iter().any(mounted))
}
fn active_mapping(v: &serde_json::Value) -> bool {
    v["children"].as_array().is_some_and(|children| {
        children.iter().any(|child| txt(child, "type") != "part" || active_mapping(child))
    })
}
fn live(v: &serde_json::Value) -> bool {
    v["mountpoints"].as_array().is_some_and(|a| {
        a.iter().filter_map(|p| p.as_str()).any(|p| {
            p == "/"
                || p.starts_with("/run/borea")
                || p.starts_with("/run/archiso")
                || p.starts_with("/run/live")
                || p.starts_with("/cdrom")
                || p.starts_with("/run/initramfs")
        })
    }) || v["children"].as_array().is_some_and(|a| a.iter().any(live))
}
pub fn parse_lsblk(text: &str, uefi: bool) -> Result<DiskFacts, Hint> {
    let data: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| hint("disk", format!("Cannot read disk inventory: {e}")))?;
    let blocks = data["blockdevices"]
        .as_array()
        .ok_or_else(|| hint("disk", "The disk inventory has no block devices."))?;
    let mut disks = Vec::new();
    for v in blocks.iter().filter(|v| txt(v, "type") == "disk") {
        let id = DiskId {
            logical_sector: match num(v, "log-sec") {
                0 => 512,
                n => n,
            },
            path: txt(v, "path").into(),
            serial: Some(txt(v, "serial")).filter(|s| !s.trim().is_empty()),
            size: num(v, "size"),
            model: txt(v, "model").trim().into(),
        };
        let mut parts = Vec::new();
        if let Some(children) = v["children"].as_array() {
            for p in children.iter().filter(|p| txt(p, "type") == "part") {
                let path = PathBuf::from(txt(p, "path"));
                let number = num(p, "partn") as u32;
                parts.push(Partition {
                    path,
                    number,
                    uuid: txt(p, "uuid"),
                    start: num(p, "start").saturating_mul(512),
                    size: num(p, "size"),
                    fs: txt(p, "fstype"),
                    efi: txt(p, "parttype")
                        .eq_ignore_ascii_case("c12a7328-f81f-11d2-ba4b-00a0c93ec93b"),
                    free: None,
                    bitlocker: txt(p, "fstype").eq_ignore_ascii_case("bitlocker"),
                    ntfs_checked: false,
                    ntfs_hibernated: false,
                    ntfs_dirty: false,
                    ntfs_min_size: None,
                });
            }
        }
        disks.push(Disk {
            id,
            readonly: flag(v, "ro"),
            in_use: mounted(v) || active_mapping(v),
            live: live(v),
            removable: flag(v, "rm"),
            transport: txt(v, "tran"),
            parts,
        });
    }
    Ok(DiskFacts { uefi, disks })
}
pub fn probe() -> Result<DiskFacts, Hint> {
    let out=Command::new(borea_platform::live_program("/usr/bin/lsblk")?).args(["--json","--bytes","--output","NAME,PATH,SIZE,TYPE,FSTYPE,PARTTYPE,PARTLABEL,LABEL,UUID,SERIAL,MODEL,RM,RO,MOUNTPOINTS,TRAN,PARTN,START,LOG-SEC"]).env("LC_ALL","C").output().map_err(|e|hint("disk",format!("Disk discovery failed: {e}")))?;
    if !out.status.success() {
        return Err(hint("disk", "lsblk could not inspect the disks."));
    }
    let mut facts = parse_lsblk(
        &String::from_utf8_lossy(&out.stdout),
        Path::new("/sys/firmware/efi").is_dir(),
    )?;
    for disk in &mut facts.disks {
        for part in &mut disk.parts {
            if part.efi {
                part.free = fat32_free(&part.path).ok();
            }
            if let Ok(mut f) = File::open(&part.path) {
                let mut boot = [0u8; 11];
                if f.read_exact(&mut boot).is_ok() && &boot[3..11] == b"-FVE-FS-" {
                    part.bitlocker = true;
                }
            }
            if part.fs == "ntfs" && !part.bitlocker {
                ntfs::inspect(part);
            }
        }
    }
    Ok(facts)
}
fn fat32_free(path: &Path) -> std::io::Result<u64> {
    let mut f = File::open(path)?;
    let mut boot = [0u8; 512];
    f.read_exact(&mut boot)?;
    let bytes = u64::from(u16::from_le_bytes([boot[11], boot[12]]));
    let clusters = u64::from(boot[13]);
    let reserved = u64::from(u16::from_le_bytes([boot[14], boot[15]]));
    let fats = u64::from(boot[16]);
    let fat_sectors = u64::from(u32::from_le_bytes(
        boot[36..40].try_into().map_err(|_| std::io::Error::other("Invalid FAT header"))?,
    ));
    let total = u64::from(u32::from_le_bytes(
        boot[32..36].try_into().map_err(|_| std::io::Error::other("Invalid FAT size"))?,
    ));
    if ![512, 1024, 2048, 4096].contains(&bytes)
        || clusters == 0
        || !clusters.is_power_of_two()
        || reserved == 0
        || fats == 0
        || fats > 2
        || fat_sectors == 0
        || total <= reserved + fats * fat_sectors
        || &boot[82..90] != b"FAT32   "
        || boot[510..512] != [0x55, 0xaa]
    {
        return Err(std::io::Error::other("Not a known FAT32 ESP"));
    }
    let count = (total - reserved - fats * fat_sectors) / clusters;
    if count + 2 > fat_sectors * bytes / 4 || count > 268_435_445 {
        return Err(std::io::Error::other("Invalid FAT32 cluster count"));
    }
    let flags = u16::from_le_bytes([boot[40], boot[41]]);
    let active = if flags & 0x80 != 0 { u64::from(flags & 0xf) } else { 0 };
    if active >= fats {
        return Err(std::io::Error::other("Invalid active FAT"));
    }
    f.seek(SeekFrom::Start((reserved + active * fat_sectors) * bytes + 8))?;
    // Count the allocation table instead of trusting Windows' potentially stale FSInfo cache.
    let mut remaining = count;
    let mut free = 0;
    let mut block = [0u8; 4096];
    while remaining != 0 {
        let n = remaining.min((block.len() / 4) as u64) as usize;
        f.read_exact(&mut block[..n * 4])?;
        for entry in block[..n * 4].chunks_exact(4) {
            if u32::from_le_bytes([entry[0], entry[1], entry[2], entry[3]]) & 0x0fff_ffff == 0 {
                free += 1;
            }
        }
        remaining -= n as u64;
    }
    Ok(free * clusters * bytes)
}

pub fn partition_path(disk: &Path, n: u32) -> PathBuf {
    let s = disk.to_string_lossy();
    PathBuf::from(format!(
        "{s}{}{n}",
        if s.chars().last().is_some_and(|c| c.is_ascii_digit()) { "p" } else { "" }
    ))
}
pub fn validate<'a>(
    choice: &DiskPlan,
    facts: &'a DiskFacts,
    desktop: bool,
) -> Result<&'a Disk, Hint> {
    if !facts.uefi {
        return Err(hint("disk", "Boot the live ISO in UEFI mode before installing."));
    }
    let id = choice.disk();
    let d =
        facts.disks.iter().find(|d| d.id == *id).ok_or_else(|| {
            hint("disk", "The selected disk identity has changed; select it again.")
        })?;
    if d.live || d.in_use || d.readonly {
        return Err(hint(
            "disk",
            "The live medium, mounted disks and read-only disks cannot be installation targets.",
        ));
    }
    if d.id.serial.as_deref().is_none_or(|s| s.trim().is_empty()) {
        return Err(hint(
            "disk",
            "This disk has no stable serial number; installation cannot be approved safely.",
        ));
    }
    let min = if desktop { 40 * GIB } else { 25 * GIB };
    let available = match choice {
        DiskPlan::Whole { .. } => id.size,
        DiskPlan::Alongside { linux_size, .. } => *linux_size,
    };
    if available < min {
        return Err(hint("disk", format!("This profile needs at least {} GiB.", min / GIB)));
    }
    Ok(d)
}
pub fn layout(
    choice: &DiskPlan,
    facts: &DiskFacts,
    distro: Distro,
    desktop: bool,
) -> Result<Layout, Hint> {
    let d = validate(choice, facts, desktop)?;
    let grub = matches!(distro, Distro::Debian | Distro::Fedora);
    let sector = d.id.logical_sector;
    if ![512, 4096].contains(&sector) {
        return Err(hint("disk", "Unsupported logical sector size."));
    }
    let mut changes = Vec::new();
    let mut next = 1u32;
    let mut start = 1024 * 1024 / sector;
    let mut esp_number = 1;
    let mut small_esp = false;
    let esp = match choice {
        DiskPlan::Whole { .. } => {
            changes.push(Change::Wipe);
            let path = partition_path(&d.id.path, next);
            changes.push(Change::Create {
                number: next,
                start_sector: start,
                end_sector: start + GIB / sector - 1,
                type_code: "ef00".into(),
                label: "EFI System".into(),
                path: path.clone(),
            });
            next += 1;
            start += GIB / sector;
            path
        }
        DiskPlan::Alongside { windows_part, linux_size, .. } => {
            let win = d
                .parts
                .iter()
                .find(|p| {
                    p.path == windows_part.path
                        && p.uuid == windows_part.uuid
                        && p.number == windows_part.number
                })
                .ok_or_else(|| hint("disk", "The Windows partition has changed."))?;
            if win.bitlocker {
                return Err(hint(
                    "disk",
                    "Turn off BitLocker in Windows before resizing this partition.",
                ));
            }
            if win.fs != "ntfs" || !win.ntfs_checked || win.ntfs_hibernated || win.ntfs_dirty {
                return Err(hint(
                    "disk",
                    "Shut down Windows fully, disable Fast Startup and check its NTFS filesystem before continuing.",
                ));
            }
            let minimum = win
                .ntfs_min_size
                .ok_or_else(|| hint("disk", "Windows free space could not be verified."))?;
            let new_size = win.size.checked_sub(*linux_size).ok_or_else(|| {
                hint("disk", "The requested Linux space is larger than the Windows partition.")
            })? / (1024 * 1024)
                * (1024 * 1024);
            if new_size < minimum.saturating_add(30 * GIB) {
                return Err(hint("disk", "Leave at least 30 GiB free for Windows."));
            }
            let esp = d.parts.iter().find(|p| p.efi).ok_or_else(|| {
                hint(
                    "disk",
                    "Windows needs an existing EFI System Partition for alongside installation.",
                )
            })?;
            let free = esp.free.ok_or_else(|| {
                hint("disk", "The EFI partition free space could not be verified.")
            })?;
            if free < 32 * 1024 * 1024 {
                return Err(hint(
                    "disk",
                    "The EFI partition needs at least 32 MiB free for the signed boot loader.",
                ));
            }
            small_esp = !grub && free < 250 * 1024 * 1024;
            esp_number = esp.number;
            next = d.parts.iter().map(|p| p.number).max().unwrap_or(0) + 1;
            start = (win.start + new_size).div_ceil(1024 * 1024) * (1024 * 1024 / sector);
            let end = (win.start + new_size) / sector - 1;
            changes.push(Change::ResizeNtfs {
                path: win.path.clone(),
                new_size,
                number: win.number,
                end_sector: end,
            });
            esp.path.clone()
        }
    };
    if next > 126 {
        return Err(hint(
            "disk",
            "There are not enough free GPT partition entries for this layout.",
        ));
    }
    let boot_fs = if grub { "ext4" } else { "vfat" };
    let boot = if grub || small_esp {
        let path = partition_path(&d.id.path, next);
        changes.push(Change::Create {
            number: next,
            start_sector: start,
            end_sector: start + GIB / sector - 1,
            type_code: if grub { "8300" } else { "ea00" }.into(),
            label: "Linux boot".into(),
            path: path.clone(),
        });
        next += 1;
        start += GIB / sector;
        Some(path)
    } else {
        None
    };
    let last = match choice {
        DiskPlan::Whole { .. } => d.id.size / sector - 34,
        DiskPlan::Alongside { windows_part, .. } => {
            let win = d
                .parts
                .iter()
                .find(|p| p.path == windows_part.path)
                .ok_or_else(|| hint("disk", "Windows partition disappeared."))?;
            (win.start + win.size) / sector - 1
        }
    };
    if last <= start + 20 * GIB / sector {
        return Err(hint("disk", "The calculated root partition is too small."));
    }
    let root = partition_path(&d.id.path, next);
    changes.push(Change::Create {
        number: next,
        start_sector: start,
        end_sector: last,
        type_code: "8309".into(),
        label: "borea_root".into(),
        path: root.clone(),
    });
    let subvolumes = match distro {
        Distro::Arch => vec![
            ("@", "/"),
            ("@home", "/home"),
            ("@var_log", "/var/log"),
            ("@cache", "/var/cache"),
            ("@snapshots", "/.snapshots"),
        ],
        Distro::Debian => vec![("@", "/"), ("@home", "/home"), ("@var_log", "/var/log")],
        Distro::Fedora => vec![("root", "/"), ("home", "/home")],
        Distro::OpenSuse => vec![
            ("@", "/"),
            ("@/home", "/home"),
            ("@/root", "/root"),
            ("@/opt", "/opt"),
            ("@/srv", "/srv"),
            ("@/usr/local", "/usr/local"),
            ("@/var", "/var"),
            ("@/.snapshots", "/.snapshots"),
        ],
    };
    let esp_uuid = d.parts.iter().find(|p| p.path == esp).map(|p| p.uuid.clone());
    let esp_size = d.parts.iter().find(|p| p.path == esp).map_or(GIB, |p| p.size);
    Ok(Layout {
        disk: d.id.clone(),
        changes,
        esp,
        esp_mount: if grub {
            "/boot/efi"
        } else if small_esp {
            "/efi"
        } else {
            "/boot"
        }
        .into(),
        esp_uuid,
        esp_size,
        esp_number,
        boot_fs: boot_fs.into(),
        boot,
        root,
        root_subvol: subvolumes[0].0.into(),
        subvolumes: subvolumes.into_iter().map(|(a, b)| (a.into(), b.into())).collect(),
    })
}
