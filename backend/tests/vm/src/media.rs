//! A genuine signed shim → GRUB → Debian kernel chain for the disposable live guest.
use std::{fs, path::Path};
pub fn secure_live_media() -> Result<(), String> {
    let root = Path::new("/work/live-esp");
    for directory in ["EFI/BOOT", "EFI/debian"] {
        fs::create_dir_all(root.join(directory)).map_err(|e| e.to_string())?;
    }
    for (source, destination) in [
        ("/usr/lib/shim/shimx64.efi.signed", "EFI/BOOT/BOOTX64.EFI"),
        ("/usr/lib/shim/mmx64.efi.signed", "EFI/BOOT/mmx64.efi"),
        ("/usr/lib/grub/x86_64-efi-signed/grubx64.efi.signed", "EFI/BOOT/grubx64.efi"),
        ("/work/vmlinuz", "borea-live-kernel"),
        ("/work/live.cpio.gz", "borea-live-initrd"),
    ] {
        fs::copy(source, root.join(destination))
            .map_err(|e| format!("Secure live media needs {source}: {e}"))?;
    }
    let config = "serial --unit=0 --speed=115200\nterminal_input serial\nterminal_output serial\nset timeout=0\nsearch --no-floppy --file --set=root /borea-live-kernel\nmenuentry 'Borea disposable signed live environment' {\n linux /borea-live-kernel console=ttyS0,115200 rdinit=/init panic=-1 tsc=reliable\n initrd /borea-live-initrd\n}\n";
    for path in ["EFI/BOOT/grub.cfg", "EFI/debian/grub.cfg"] {
        fs::write(root.join(path), config).map_err(|e| e.to_string())?;
    }
    Ok(())
}
