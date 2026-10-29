//! The writable data partition after Borea Live on the stick.
//!
//! The ISO is read-only, so whatever Borea Live downloads to hand over to another
//! installer (Omarchy's ISO, its unattended answers) lives in an ext4 partition
//! filling the rest of the stick. Borea Live finds it as partition 4 of its own
//! medium. Omarchy's installer finds its answers by the filesystem label
//! `cidata`, which Borea Live sets only for an unattended hand-off.
//!
//! The ISO's partition table is a plain MBR (Limine's `bios-install` converts the
//! hybrid ISO's GPT to MBR) with three entries, so entry 4 is free.

/// The filesystem label of the data partition when nothing is being handed off.
pub const LABEL: &str = "BOREA_DATA";
/// Smaller than this and the space is not worth a partition.
pub const MINIMUM: u64 = 1 << 30;
const ALIGN: u64 = 1 << 20;
const TABLE: usize = 446;
const LINUX: u8 = 0x83;

/// Where the data partition goes, in sectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// MBR slot, 1-based: always 4 for a Borea Live stick.
    pub number: u32,
    pub start: u64,
    pub sectors: u64,
}

/// Why no data partition can be added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotMbr,
    NoFreeSlot,
    TooSmall { free: u64 },
    Overlaps,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NotMbr => f.write_str("the stick's partition table is not the image's MBR"),
            Refusal::NoFreeSlot => f.write_str("the image uses every MBR partition slot"),
            Refusal::TooSmall { free } => write!(
                f,
                "only {} is left after the image; Borea needs at least {} for other installers",
                crate::ui::human(*free),
                crate::ui::human(MINIMUM)
            ),
            Refusal::Overlaps => f.write_str("the free space overlaps an existing partition"),
        }
    }
}

fn entry(mbr: &[u8; 512], slot: usize) -> &[u8] {
    &mbr[TABLE + slot * 16..TABLE + slot * 16 + 16]
}

fn u32_at(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]))
}

/// The partition to add after an image of `image` bytes on a disk of `disk`
/// bytes with `sector`-byte sectors: from the first MiB boundary after the
/// image to the end of the disk.
pub fn plan(mbr: &[u8; 512], image: u64, disk: u64, sector: u64) -> Result<Entry, Refusal> {
    if mbr[510..512] != [0x55, 0xAA] {
        return Err(Refusal::NotMbr);
    }
    let slot =
        (0..4).find(|&s| entry(mbr, s).iter().all(|&b| b == 0)).ok_or(Refusal::NoFreeSlot)?;
    let start_byte = image.div_ceil(ALIGN) * ALIGN;
    // MBR addresses at most 2^32 sectors; a bigger stick keeps the rest unused.
    let end_byte = disk.min((u64::from(u32::MAX) + 1) * sector) / ALIGN * ALIGN;
    let free = end_byte.saturating_sub(start_byte);
    if free < MINIMUM {
        return Err(Refusal::TooSmall { free });
    }
    let (start, sectors) = (start_byte / sector, free / sector);
    for s in (0..4).filter(|&s| s != slot) {
        let e = entry(mbr, s);
        let (first, count) = (u32_at(e, 8), u32_at(e, 12));
        if count > 0 && first < start + sectors && start < first + count {
            return Err(Refusal::Overlaps);
        }
    }
    Ok(Entry { number: slot as u32 + 1, start, sectors })
}

/// Write `entry` into the MBR as a Linux partition.
pub fn add(mbr: &mut [u8; 512], entry: Entry) {
    let at = TABLE + (entry.number as usize - 1) * 16;
    let e = &mut mbr[at..at + 16];
    e[0] = 0x00; // not bootable: the image's own partition stays the active one
    e[1..4].copy_from_slice(&[0xFE, 0xFF, 0xFF]); // CHS: beyond 8 GiB, use LBA
    e[4] = LINUX;
    e[5..8].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    e[8..12].copy_from_slice(&(entry.start as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(entry.sectors as u32).to_le_bytes());
}

/// The device name of partition `n` of disk `name`: sdb4, nvme0n1p4, mmcblk0p4.
pub fn partition_name(name: &str, n: u32) -> String {
    let p = if name.ends_with(|c: char| c.is_ascii_digit()) { "p" } else { "" };
    format!("{name}{p}{n}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The MBR Limine leaves on a Borea Live ISO: three entries, as measured on
    /// a 304 MiB image built with hooks::compose_image's xorriso flags.
    fn iso_mbr() -> [u8; 512] {
        let mut mbr = [0u8; 512];
        mbr[510] = 0x55;
        mbr[511] = 0xAA;
        for (slot, (status, kind, start, count)) in
            [(0x80u8, 0x07u8, 64u32, 76u32), (0, 0xEF, 140, 6144), (0, 0x07, 6284, 616480)]
                .into_iter()
                .enumerate()
        {
            let at = TABLE + slot * 16;
            mbr[at] = status;
            mbr[at + 4] = kind;
            mbr[at + 8..at + 12].copy_from_slice(&start.to_le_bytes());
            mbr[at + 12..at + 16].copy_from_slice(&count.to_le_bytes());
        }
        mbr
    }

    const IMAGE: u64 = 318_887_936; // 622764 sectors

    #[test]
    fn fills_the_stick_after_the_image_on_a_mib_boundary() {
        let mbr = iso_mbr();
        let e = plan(&mbr, IMAGE, 16_000_000_000, 512).unwrap();
        assert_eq!(e.number, 4);
        assert_eq!(e.start * 512 % (1 << 20), 0);
        assert!(e.start * 512 >= IMAGE);
        assert!((e.start + e.sectors) * 512 <= 16_000_000_000);

        let mut written = mbr;
        add(&mut written, e);
        assert_eq!(written[..TABLE], mbr[..TABLE], "the boot code is untouched");
        assert_eq!(written[TABLE..TABLE + 48], mbr[TABLE..TABLE + 48], "the image's entries too");
        let fourth = &written[TABLE + 48..TABLE + 64];
        assert_eq!(fourth[0], 0, "the image's partition stays the active one");
        assert_eq!(fourth[4], LINUX);
        assert_eq!(u32_at(fourth, 8), e.start);
        assert_eq!(u32_at(fourth, 12), e.sectors);
        assert_eq!(written[510..], [0x55, 0xAA]);
    }

    #[test]
    fn refuses_what_it_cannot_do_safely() {
        let mbr = iso_mbr();
        assert!(matches!(
            plan(&mbr, IMAGE, IMAGE + (500 << 20), 512),
            Err(Refusal::TooSmall { .. })
        ));
        let mut full = mbr;
        add(&mut full, plan(&mbr, IMAGE, 8_000_000_000, 512).unwrap());
        assert_eq!(plan(&full, IMAGE, 8_000_000_000, 512), Err(Refusal::NoFreeSlot));
        let mut blank = mbr;
        blank[510] = 0;
        assert_eq!(plan(&blank, IMAGE, 8_000_000_000, 512), Err(Refusal::NotMbr));
        // An image entry reaching past the image would be overwritten.
        let mut odd = mbr;
        odd[TABLE + 32 + 12..TABLE + 32 + 16].copy_from_slice(&4_000_000u32.to_le_bytes());
        assert_eq!(plan(&odd, IMAGE, 8_000_000_000, 512), Err(Refusal::Overlaps));
    }

    #[test]
    fn a_huge_stick_stops_at_the_mbr_limit() {
        let e = plan(&iso_mbr(), IMAGE, 4 << 40, 512).unwrap();
        assert!(e.start + e.sectors <= u64::from(u32::MAX) + 1);
    }

    #[test]
    fn names_partitions_like_the_kernel() {
        assert_eq!(partition_name("sdb", 4), "sdb4");
        assert_eq!(partition_name("nvme0n1", 4), "nvme0n1p4");
        assert_eq!(partition_name("mmcblk0", 4), "mmcblk0p4");
    }

    /// `BOREA_DATAPART_ISO=path/to/borea-live.iso cargo test -p borea-flash real_iso`:
    /// put a real image on a disposable 8 GB stick image, add and format the data
    /// partition, and check it with the standard tools. CI runs it on the
    /// composed ISO and boots the result; it never touches a real disk.
    #[test]
    fn real_iso() {
        use std::os::unix::fs::FileExt;
        use std::process::Command;
        let Some(iso) = std::env::var_os("BOREA_DATAPART_ISO") else { return };
        let out = std::env::var_os("BOREA_DATAPART_STICK")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("borea-datapart-stick.img"));
        let size: u64 = std::env::var("BOREA_DATAPART_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8_000_000_000);
        let image = std::fs::read(&iso).unwrap();
        let stick = std::fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&out)
            .unwrap();
        stick.set_len(size).unwrap();
        stick.write_all_at(&image, 0).unwrap();
        let mut mbr = [0u8; 512];
        mbr.copy_from_slice(&image[..512]);
        let entry = plan(&mbr, image.len() as u64, size, 512).unwrap();
        add(&mut mbr, entry);
        stick.write_all_at(&mbr, 0).unwrap();
        stick.sync_all().unwrap();

        let table = Command::new("sfdisk").args(["-d"]).arg(&out).output().unwrap();
        let table = String::from_utf8_lossy(&table.stdout);
        assert!(
            table.contains(&format!(
                "start={:>12}, size={:>12}, type=83",
                entry.start, entry.sectors
            )),
            "{table}"
        );
        let offset = format!("offset={}", entry.start * 512);
        let size = (entry.sectors * 512 / 1024).to_string();
        let made = Command::new("mkfs.ext4")
            .args(["-q", "-F", "-L", LABEL, "-E", &offset])
            .arg(&out)
            .arg(&size)
            .status()
            .unwrap();
        assert!(made.success());
        let seen = Command::new("blkid")
            .args(["-p", "-O", &(entry.start * 512).to_string(), "-s", "LABEL", "-o", "value"])
            .arg(&out)
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&seen.stdout).trim(), LABEL);
        // The image itself is byte for byte what was written, apart from entry 4.
        let mut back = vec![0u8; image.len()];
        stick.read_exact_at(&mut back, 0).unwrap();
        assert_eq!(back[512..], image[512..]);
        assert_eq!(back[..TABLE + 48], image[..TABLE + 48]);
    }
}
