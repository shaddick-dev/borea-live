//! Secure Boot for the installed system: an opt-in, never the default.
//!
//! When chosen, Borea uses the machine owner's own keys rather than anyone else's:
//! it signs Limine, enrolls the config hash so Limine checks every kernel and
//! initramfs, keeps Microsoft's keys so graphics and network option ROMs still load,
//! and backs up whatever keys the firmware holds today. Nothing is enforced until
//! the firmware's own Secure Boot switch is on, and Borea checks the first boot.

use borea_hardware::HardwareSnapshot;
use borea_live_core::{Fact, FirmwareMode};

use crate::{Acquisition, AdapterSpec};

/// What choosing Secure Boot would do on this machine, for this target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// Can be chosen. Keys go in automatically when the firmware is in Setup Mode;
    /// otherwise Borea leaves its certificate on the EFI partition to enroll from
    /// firmware setup.
    Available { enrollment: Enrollment },
    /// Cannot be chosen, with the reason.
    Unavailable(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enrollment {
    /// The firmware has no platform key, so it takes new keys without a password.
    Automatic,
    /// Keys are already enrolled; the user adds Borea's certificate from firmware setup.
    FromFirmwareSetup,
}

/// The steps Borea takes when Secure Boot is chosen, as shown before approval.
pub const STEPS: [&str; 5] = [
    "Create this machine's own Secure Boot keys on the new system, readable by root only.",
    "Sign Limine and enroll its config hash; every kernel and initramfs carries a hash Limine checks, renewed on each kernel update.",
    "Keep Microsoft's keys alongside, so graphics and network cards' firmware still loads.",
    "Back up the firmware's current keys to the EFI partition before changing anything.",
    "Leave the firmware switch to you, then confirm the new system boots with Secure Boot on.",
];

pub fn readiness(spec: &AdapterSpec, hw: &HardwareSnapshot) -> Readiness {
    let unavailable = |why: &str| Readiness::Unavailable(why.into());
    match &hw.firmware {
        Fact::Known(FirmwareMode::Uefi { bits: Some(64) | None }) => {}
        Fact::Known(FirmwareMode::Uefi { .. }) => {
            return unavailable("32-bit UEFI is not supported");
        }
        Fact::Known(FirmwareMode::Bios) => {
            return unavailable("this machine boots in legacy BIOS mode");
        }
        Fact::Unknown { .. } => return unavailable("the firmware mode could not be read"),
    }
    if spec.acquisition == Acquisition::PendingVerification {
        return unavailable("the upstream boot path has not been verified");
    }
    if let Acquisition::ImageDeployment { .. } = spec.acquisition {
        return unavailable(
            "this edition's image-based boot path is not certified with Secure Boot yet",
        );
    }
    let enrollment = match hw.setup_mode {
        Fact::Known(true) => Enrollment::Automatic,
        _ => Enrollment::FromFirmwareSetup,
    };
    Readiness::Available { enrollment }
}

/// A warning when the choice and the firmware disagree in a way that would stop the
/// new system booting.
pub fn conflict(chosen: bool, hw: &HardwareSnapshot) -> Option<&'static str> {
    (!chosen && hw.secure_boot == Fact::Known(true)).then_some(
        "Secure Boot is on in this machine's firmware. Without it on the new system, turn it off in firmware setup before the first boot, or choose On.",
    )
}

impl Enrollment {
    pub fn describe(self) -> &'static str {
        match self {
            Enrollment::Automatic => {
                "The firmware is in Setup Mode, so Borea can enroll the keys itself."
            }
            Enrollment::FromFirmwareSetup => {
                "The firmware already holds keys. Borea leaves its certificate on the EFI partition for you to enroll from firmware setup, or you can clear the keys to let Borea do it."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter;
    use borea_live_core::DeviceClass;

    fn machine(firmware: FirmwareMode, secure_boot: bool, setup_mode: bool) -> HardwareSnapshot {
        HardwareSnapshot {
            cpu: Fact::unknown("not needed"),
            memory_bytes: Fact::unknown("not needed"),
            firmware: Fact::Known(firmware),
            secure_boot: Fact::Known(secure_boot),
            setup_mode: Fact::Known(setup_mode),
            device_class: Fact::Known(DeviceClass::Desktop),
            gpus: vec![],
            displays: vec![],
            disks: vec![],
            network: vec![],
            bluetooth: vec![],
            inputs: vec![],
            battery: false,
        }
    }

    const UEFI: FirmwareMode = FirmwareMode::Uefi { bits: Some(64) };

    #[test]
    fn setup_mode_decides_how_keys_are_enrolled() {
        let arch = adapter("arch").unwrap();
        assert_eq!(
            readiness(arch, &machine(UEFI, false, true)),
            Readiness::Available { enrollment: Enrollment::Automatic }
        );
        assert_eq!(
            readiness(arch, &machine(UEFI, false, false)),
            Readiness::Available { enrollment: Enrollment::FromFirmwareSetup }
        );
    }

    #[test]
    fn unavailable_where_it_cannot_work_safely() {
        let arch = adapter("arch").unwrap();
        assert!(matches!(
            readiness(arch, &machine(FirmwareMode::Bios, false, false)),
            Readiness::Unavailable(_)
        ));
        let butterbian = adapter("butterbian").unwrap();
        assert!(matches!(
            readiness(butterbian, &machine(UEFI, false, true)),
            Readiness::Unavailable(_)
        ));
        let bazzite = adapter("bazzite").unwrap();
        assert!(
            matches!(readiness(bazzite, &machine(UEFI, false, true)), Readiness::Unavailable(r) if r.contains("not certified"))
        );
    }

    #[test]
    fn warns_when_firmware_would_refuse_an_unsigned_system() {
        assert!(conflict(false, &machine(UEFI, true, false)).is_some());
        assert!(conflict(true, &machine(UEFI, true, false)).is_none());
        assert!(conflict(false, &machine(UEFI, false, false)).is_none());
    }
}
