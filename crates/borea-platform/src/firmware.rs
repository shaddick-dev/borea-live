//! UEFI's OS Indications support, read without changing firmware variables.
use std::path::Path;

pub fn setup_supported(root: &Path) -> bool {
    let path = root.join(
        "sys/firmware/efi/efivars/OsIndicationsSupported-8be4df61-93ca-11d2-aa0d-00e098032b8c",
    );
    std::fs::read(path).is_ok_and(|bytes| supports_setup(&bytes))
}

fn supports_setup(bytes: &[u8]) -> bool {
    // efivarfs prefixes the UINT64 payload with four attribute bytes.
    let Some(value) = bytes.get(4..).and_then(|b| <[u8; 8]>::try_from(b).ok()) else {
        return false;
    };
    u64::from_le_bytes(value) & 1 != 0 // EFI_OS_INDICATIONS_BOOT_TO_FW_UI
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_ui_requires_its_own_bit_and_complete_variable() {
        let mut bytes = vec![7, 0, 0, 0];
        bytes.extend_from_slice(&2_u64.to_le_bytes());
        assert!(!supports_setup(&bytes));
        bytes[4] |= 1;
        assert!(supports_setup(&bytes));
        for length in 0..12 {
            assert!(!supports_setup(&bytes[..length]));
        }
        assert!(!setup_supported(Path::new("/nonexistent/borea-fixture")));
    }
}
