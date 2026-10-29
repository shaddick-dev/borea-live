//! The Borea stick's data partition, found or made by Borea Live itself.
//!
//! borea-rataTUI adds it when it writes the stick. A stick written any other way
//! (dd, Etcher, an older rataTUI) has only the image, so Borea Live adds the same
//! partition itself: MBR entry 4, from the first MiB after the image to the end
//! of the stick (capped at the MBR's reach), never overlapping the image.

const TABLE: usize = 446;
const ALIGN: u64 = 1 << 20;
/// Smaller than this and there is no point adding it.
pub const MINIMUM: u64 = 1 << 30;

/// The image's exact size, from its ISO 9660 primary volume descriptor (the
/// 2048 bytes at offset 32768): volume space size × logical block size.
pub fn iso_size(pvd: &[u8]) -> Option<u64> {
    if pvd.len() < 130 || pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return None;
    }
    let blocks = u32::from_le_bytes([pvd[80], pvd[81], pvd[82], pvd[83]]);
    let block = u16::from_le_bytes([pvd[128], pvd[129]]);
    (blocks > 0 && block > 0).then(|| u64::from(blocks) * u64::from(block))
}

fn entry(mbr: &[u8; 512], slot: usize) -> (u8, u64, u64) {
    let e = &mbr[TABLE + slot * 16..TABLE + slot * 16 + 16];
    let le = |at: usize| u64::from(u32::from_le_bytes([e[at], e[at + 1], e[at + 2], e[at + 3]]));
    (e[4], le(8), le(12))
}

/// Entry 4, as (start sector, sectors), when the stick already has one.
pub fn existing(mbr: &[u8; 512]) -> Option<(u64, u64)> {
    let (kind, start, sectors) = entry(mbr, 3);
    (mbr[510..512] == [0x55, 0xAA] && kind != 0 && sectors > 0).then_some((start, sectors))
}

/// Where to add entry 4, as (start sector, sectors).
pub fn plan(mbr: &[u8; 512], image: u64, disk: u64, sector: u64) -> Result<(u64, u64), String> {
    if mbr[510..512] != [0x55, 0xAA] {
        return Err("the stick does not start with the Borea Live image".into());
    }
    if existing(mbr).is_some() {
        return Err("the stick's fourth partition is already in use".into());
    }
    let start = image.div_ceil(ALIGN) * ALIGN;
    let end = disk.min((u64::from(u32::MAX) + 1) * sector) / ALIGN * ALIGN;
    let free = end.saturating_sub(start);
    if free < MINIMUM {
        return Err(format!(
            "the stick has only {} MB left after Borea Live; use a larger stick",
            free / 1_000_000
        ));
    }
    let (first, count) = (start / sector, free / sector);
    for slot in 0..3 {
        let (kind, s, n) = entry(mbr, slot);
        if kind != 0 && n > 0 && s < first + count && first < s + n {
            return Err("the free space overlaps a partition of the image".into());
        }
    }
    Ok((first, count))
}

/// Write entry 4 as a Linux partition. The image's own entries and boot code stay.
pub fn add(mbr: &mut [u8; 512], start: u64, sectors: u64) {
    let e = &mut mbr[TABLE + 48..TABLE + 64];
    e[0] = 0;
    e[1..4].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    e[4] = 0x83;
    e[5..8].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
    e[8..12].copy_from_slice(&(start as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(sectors as u32).to_le_bytes());
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// A Borea Live image's MBR as Limine leaves it: three entries.
    fn image_mbr() -> [u8; 512] {
        let mut mbr = [0u8; 512];
        mbr[510] = 0x55;
        mbr[511] = 0xAA;
        for (slot, (kind, start, count)) in
            [(0x07u8, 64u32, 84u32), (0xEF, 148, 6144), (0x07, 6292, 1_409_284)]
                .into_iter()
                .enumerate()
        {
            let at = TABLE + slot * 16;
            mbr[at + 4] = kind;
            mbr[at + 8..at + 12].copy_from_slice(&start.to_le_bytes());
            mbr[at + 12..at + 16].copy_from_slice(&count.to_le_bytes());
        }
        mbr
    }

    fn pvd(blocks: u32) -> Vec<u8> {
        let mut d = vec![0u8; 2048];
        d[0] = 1;
        d[1..6].copy_from_slice(b"CD001");
        d[80..84].copy_from_slice(&blocks.to_le_bytes());
        d[128..130].copy_from_slice(&2048u16.to_le_bytes());
        d
    }

    #[test]
    fn reads_the_image_size_from_its_volume_descriptor() {
        assert_eq!(iso_size(&pvd(354_000)), Some(354_000 * 2048));
        assert_eq!(iso_size(&[0u8; 2048]), None);
    }

    #[test]
    fn adds_the_rest_of_a_plain_stick_once() {
        let mut mbr = image_mbr();
        assert_eq!(existing(&mbr), None);
        let image = 354_000 * 2048;
        let (start, sectors) = plan(&mbr, image, 32_000_000_000, 512).unwrap();
        assert!(start * 512 >= image && start * 512 % ALIGN == 0);
        assert!((start + sectors) * 512 <= 32_000_000_000);
        let before = mbr;
        add(&mut mbr, start, sectors);
        assert_eq!(mbr[..TABLE + 48], before[..TABLE + 48], "boot code and image entries stay");
        assert_eq!(existing(&mbr), Some((start, sectors)));
        assert!(plan(&mbr, image, 32_000_000_000, 512).is_err(), "never twice");
    }

    #[test]
    fn refuses_small_sticks_and_odd_tables() {
        let mbr = image_mbr();
        let image = 354_000 * 2048;
        let small = plan(&mbr, image, image + (300 << 20), 512).unwrap_err();
        assert!(small.contains("larger stick"), "{small}");
        let mut blank = mbr;
        blank[510] = 0;
        assert!(plan(&blank, image, 32_000_000_000, 512).is_err());
        let mut odd = mbr;
        odd[TABLE + 32 + 12..TABLE + 32 + 16].copy_from_slice(&40_000_000u32.to_le_bytes());
        assert!(plan(&odd, image, 32_000_000_000, 512).unwrap_err().contains("overlaps"));
    }
}
