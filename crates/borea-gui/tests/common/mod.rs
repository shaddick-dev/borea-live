//! Fixtures shared by the window tests.

use borea_hardware::{Cpu, Disk, DiskTransport, DiskUsage, Gpu, GpuVendor, HardwareSnapshot};
use borea_live_core::{Architecture, DeviceClass, Fact, FirmwareMode, X86Level};

/// A desktop with one free disk and one that runs the live session.
pub fn machine() -> HardwareSnapshot {
    HardwareSnapshot {
        cpu: Fact::Known(Cpu {
            architecture: Architecture::X86_64,
            model: Some("AMD Ryzen 7 7800X3D".into()),
            vendor: Some("AuthenticAMD".into()),
            x86_level: Some(X86Level::V4),
            logical_cores: 16,
        }),
        memory_bytes: Fact::Known(32_000_000_000),
        firmware: Fact::Known(FirmwareMode::Uefi { bits: Some(64) }),
        secure_boot: Fact::Known(false),
        setup_mode: Fact::Known(false),
        device_class: Fact::Known(DeviceClass::Desktop),
        gpus: vec![Gpu {
            pci_slot: "0000:03:00.0".into(),
            vendor: GpuVendor::Amd,
            device_id: 0x747e,
            host_driver: Some("amdgpu".into()),
            boot_display: true,
        }],
        displays: vec![],
        disks: vec![
            Disk {
                name: "nvme0n1".into(),
                model: Some("Example NVMe".into()),
                size_bytes: 1_000_204_886_016,
                removable: false,
                rotational: Some(false),
                transport: DiskTransport::Nvme,
                partitions: vec![],
                usage: DiskUsage::Free,
            },
            Disk {
                name: "sda".into(),
                model: Some("USB stick".into()),
                size_bytes: 62_000_000_000,
                removable: true,
                rotational: None,
                transport: DiskTransport::Usb,
                partitions: vec![],
                usage: DiskUsage::InstallationMedium,
            },
        ],
        network: vec![],
        bluetooth: vec![],
        inputs: vec![],
        battery: false,
    }
}
