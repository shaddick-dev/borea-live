//! Conservative read-only inspection of NTFS $LogFile restart pages.
//! Field layout: upstream ntfs-3g's include/ntfs-3g/logfile.h. Unsupported,
//! torn or inconsistent records return unknown and block resizing.
fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}
fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}
fn u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?))
}
fn restart(bytes: &[u8], offset: usize, total: usize) -> Option<(u64, bool, usize)> {
    let header = bytes.get(offset..offset + 32)?;
    if &header[..4] != b"RSTR"
        || u16_at(header, 28)? != 1
        || u16_at(header, 26)? != 1
        || u64_at(header, 8)? != 0
    {
        return None;
    }
    let size = u32_at(header, 16)? as usize;
    let log_size = u32_at(header, 20)? as usize;
    if !(512..=65536).contains(&size)
        || !size.is_power_of_two()
        || !(512..=65536).contains(&log_size)
        || !log_size.is_power_of_two()
        || offset != 0 && offset != size
    {
        return None;
    }
    let mut page = bytes.get(offset..offset + size)?.to_vec();
    let usa = u16_at(&page, 4)? as usize;
    let count = u16_at(&page, 6)? as usize;
    let area = u16_at(&page, 24)? as usize;
    if count != size / 512 + 1
        || usa < 30
        || usa + count * 2 > 510
        || area < usa + count * 2
        || area % 8 != 0
        || area + 48 > 510
    {
        return None;
    }
    let sequence = u16_at(&page, usa)?;
    for sector in 1..count {
        let end = sector * 512 - 2;
        if u16_at(&page, end)? != sequence {
            return None;
        }
        let replacement = u16_at(&page, usa + sector * 2)?.to_le_bytes();
        page.get_mut(end..end + 2)?.copy_from_slice(&replacement);
    }
    let clients = u16_at(&page, area + 8)?;
    let free = u16_at(&page, area + 10)?;
    let used = u16_at(&page, area + 12)?;
    let length = u16_at(&page, area + 20)? as usize;
    let client_offset = u16_at(&page, area + 22)? as usize;
    let file_size = u64_at(&page, area + 24)?;
    if clients != 1
        || ![0, u16::MAX].contains(&free)
        || ![0, u16::MAX].contains(&used)
        || client_offset < 48
        || client_offset % 8 != 0
        || area + client_offset > 510
        || client_offset + 160 > length
        || area + length > size
        || file_size > total as u64
        || file_size < (50 * log_size) as u64
        || u32_at(&page, area + 16)? != 67 - (64 - file_size.leading_zeros())
        || u16_at(&page, area + 36)? % 8 != 0
        || u16_at(&page, area + 38)? % 8 != 0
    {
        return None;
    }
    let dirty = used != u16::MAX && u16_at(&page, area + 14)? & 2 == 0;
    Some((u64_at(&page, area)?, dirty, size))
}
pub(crate) fn dirty(bytes: &[u8]) -> Option<bool> {
    // The entire bounded attribute must have been read; a prefix cannot prove erasure.
    if bytes.len() >= 50 * 4096 && bytes.iter().all(|byte| *byte == 255) {
        return Some(false);
    }
    let first = restart(bytes, 0, bytes.len())?;
    let second = restart(bytes, first.2, bytes.len())?;
    if first.2 != second.2 {
        return None;
    }
    Some(if second.0 > first.0 { second.1 } else { first.1 })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn latest_complete_restart_controls_clean_state_and_torn_pages_are_unknown() {
        let mut bytes = vec![0; 256 * 1024];
        for offset in [0, 4096] {
            let page = &mut bytes[offset..offset + 4096];
            page[..4].copy_from_slice(b"RSTR");
            for (at, value) in [
                (4, 30u16),
                (6, 9),
                (24, 48),
                (26, 1),
                (28, 1),
                (30, 23),
                (56, 1),
                (58, u16::MAX),
                (60, 0),
                (62, 2),
                (68, 224),
                (70, 64),
                (84, 48),
                (86, 64),
            ] {
                page[at..at + 2].copy_from_slice(&value.to_le_bytes());
            }
            page[16..20].copy_from_slice(&4096u32.to_le_bytes());
            page[20..24].copy_from_slice(&4096u32.to_le_bytes());
            page[64..68].copy_from_slice(&48u32.to_le_bytes());
            page[72..80].copy_from_slice(&(256 * 1024u64).to_le_bytes());
            page[48..56].copy_from_slice(&(offset as u64 + 1).to_le_bytes());
            for sector in 1..=8 {
                page[sector * 512 - 2..sector * 512].copy_from_slice(&23u16.to_le_bytes());
            }
        }
        assert_eq!(dirty(&bytes), Some(false));
        bytes[4096 + 62] = 0;
        assert_eq!(dirty(&bytes), Some(true));
        bytes[4096 + 510] = 0;
        assert_eq!(dirty(&bytes), None);
        assert_eq!(dirty(&vec![255; 256 * 1024]), Some(false));
        assert_eq!(dirty(&[255; 512]), None);
    }
}
