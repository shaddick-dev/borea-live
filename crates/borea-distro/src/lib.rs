//! Distribution adapters: how each target system is obtained, independently of the host.
//!
//! An adapter never assumes the host shares its distribution. It brings the target's
//! native tools with it, from the target's own upstream sources: a bootstrap archive,
//! a container image, an OS image, or an installation image used purely as a download
//! source (never booted). The host contributes only kernel and platform capabilities.
//!
//! Acquisition strategies recorded here are the M0 research candidates for each target.
//! None is implemented yet; every catalog entry stays non-installable until its
//! adapter passes certification.

pub mod secure_boot;

use std::fmt;

use borea_catalog::Distro;
use borea_core::{Architecture, Fact, FirmwareMode, SupportStatus};
use borea_hardware::{DiskUsage, GpuVendor, HardwareSnapshot, Microcode};
use borea_host::{HostCapability, HostEnvironment};

/// Where the target's native package tooling comes from. Never the host's PATH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toolchain {
    /// A signed upstream bootstrap root filesystem archive.
    UpstreamBootstrapArchive(&'static str),
    /// An upstream container image of the target, unpacked and run in a namespace.
    UpstreamContainerImage(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acquisition {
    /// Bootstrap from the target's repositories with its own package manager.
    PackageBootstrap { manager: &'static str, toolchain: Toolchain },
    /// Install the parent target, then apply the derivative's upstream layer.
    /// The derivative still resolves its own release channel.
    Layered { base: &'static str, layer: &'static str },
    /// Deploy an upstream OS image natively, preserving its update and rollback model.
    ImageDeployment { image: &'static str },
    /// Take the root filesystem from an upstream installation image used as a
    /// download source. The image is verified and read, never booted.
    UpstreamImageSource { image: &'static str },
}

impl fmt::Display for Toolchain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Toolchain::UpstreamBootstrapArchive(what) | Toolchain::UpstreamContainerImage(what) => {
                f.write_str(what)
            }
        }
    }
}

impl fmt::Display for Acquisition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Acquisition::PackageBootstrap { manager, toolchain } => {
                write!(f, "{manager} bootstrap, tools from {toolchain}")
            }
            Acquisition::Layered { base, layer } => write!(f, "{base} base + {layer}"),
            Acquisition::ImageDeployment { image } => write!(f, "native deployment of {image}"),
            Acquisition::UpstreamImageSource { image } => write!(f, "root filesystem from {image}"),
        }
    }
}

/// What every UEFI target needs from any host: an isolated mount tree, an ESP, and
/// access to boot variables for the Limine entry.
const COMMON_NEEDS: &[HostCapability] =
    &[HostCapability::MountNamespaces, HostCapability::KernelVfat, HostCapability::EfiVariables];

#[derive(Debug, Clone, Copy)]
pub struct AdapterSpec {
    pub distro: &'static str,
    pub acquisition: Acquisition,
    /// Host capabilities this target needs beyond [`COMMON_NEEDS`].
    pub needs: &'static [HostCapability],
    /// Questions M0 must answer before this adapter can be built.
    pub open_questions: &'static [&'static str],
}

impl AdapterSpec {
    pub fn host_needs(&self) -> impl Iterator<Item = &HostCapability> {
        COMMON_NEEDS.iter().chain(self.needs)
    }
}

use HostCapability as H;

/// One adapter per catalog distribution.
pub const ADAPTERS: &[AdapterSpec] = &[
    AdapterSpec {
        distro: "arch",
        acquisition: Acquisition::PackageBootstrap {
            manager: "pacman",
            toolchain: Toolchain::UpstreamBootstrapArchive("the signed Arch bootstrap archive"),
        },
        needs: &[H::KernelExt4],
        open_questions: &["Run pacstrap from the bootstrap root without host keyring access."],
    },
    AdapterSpec {
        distro: "cachyos",
        acquisition: Acquisition::Layered {
            base: "Arch bootstrap",
            layer: "CachyOS repositories, keyring, and CPU-matched optimized packages",
        },
        needs: &[H::KernelExt4],
        open_questions: &["Which upstream artifacts define the Handheld edition's package set?"],
    },
    AdapterSpec {
        distro: "omarchy",
        acquisition: Acquisition::Layered {
            base: "Arch bootstrap",
            layer: "Omarchy's upstream installer at a pinned release",
        },
        needs: &[H::KernelBtrfs],
        open_questions: &[
            "Confirm Omarchy's required disk layout and encryption before enabling.",
            "Can its installer run non-interactively against a target root?",
        ],
    },
    AdapterSpec {
        distro: "fedora",
        acquisition: Acquisition::PackageBootstrap {
            manager: "dnf",
            toolchain: Toolchain::UpstreamContainerImage(
                "the Fedora container image of the resolved release",
            ),
        },
        needs: &[H::KernelBtrfs],
        open_questions: &["Apply SELinux labels to the target from a host without SELinux."],
    },
    AdapterSpec {
        distro: "bazzite",
        acquisition: Acquisition::ImageDeployment {
            image: "the Bazzite OCI image pinned by digest",
        },
        needs: &[H::KernelBtrfs, H::KernelOverlay, H::LoopDevices],
        open_questions: &[
            "Can the image's own deployment tooling run from an unpacked image root?",
            "Limine entry or native-loader handoff that survives image updates and rollback.",
        ],
    },
    AdapterSpec {
        distro: "nobara",
        acquisition: Acquisition::UpstreamImageSource {
            image: "the official Nobara image for the edition",
        },
        needs: &[H::KernelBtrfs, H::LoopDevices],
        open_questions: &[
            "Image root filesystem format (squashfs or erofs) for the current release.",
            "Is a repository bootstrap preferable to image extraction for Nobara?",
        ],
    },
    AdapterSpec {
        distro: "debian",
        acquisition: Acquisition::PackageBootstrap {
            manager: "apt (debootstrap)",
            toolchain: Toolchain::UpstreamContainerImage("the Debian stable container image"),
        },
        needs: &[H::KernelExt4],
        open_questions: &["Pin the point release and security archive identity in the plan."],
    },
    AdapterSpec {
        distro: "pikaos",
        acquisition: Acquisition::UpstreamImageSource {
            image: "the official PikaOS image for the edition and Standard/NVIDIA variant",
        },
        needs: &[H::LoopDevices],
        open_questions: &["Image root filesystem format, and the target layout PikaOS expects."],
    },
    AdapterSpec {
        distro: "tumbleweed",
        acquisition: Acquisition::PackageBootstrap {
            manager: "zypper",
            toolchain: Toolchain::UpstreamContainerImage(
                "the Tumbleweed container image at the pinned snapshot",
            ),
        },
        needs: &[H::KernelBtrfs],
        open_questions: &["Resolve the Gaming package set and firmware per snapshot and GPU."],
    },
];

pub fn adapter(distro: &str) -> Option<&'static AdapterSpec> {
    ADAPTERS.iter().find(|a| a.distro == distro)
}

/// One observation about a target on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// Prevents installation here.
    Blocker(String),
    /// Could not be determined. Shown to the user; never silently passed or failed.
    Unknown(String),
    /// A suggestion the user can accept or override.
    Recommend(String),
}

/// The overall answer for one target, from this host, on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ready,
    /// The host or hardware rules it out (see the findings).
    Blocked,
    /// Fits this machine, but Borea must run from a live environment to install.
    NeedsLiveEnvironment,
    /// Fits this machine and host; the adapter is not certified yet.
    AwaitingCertification(SupportStatus),
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Ready => f.write_str("ready"),
            Verdict::Blocked => f.write_str("blocked here"),
            Verdict::NeedsLiveEnvironment => {
                f.write_str("machine fits; start Borea from a live environment")
            }
            Verdict::AwaitingCertification(status) => {
                write!(f, "{status} · nothing blocks it here")
            }
        }
    }
}

/// Why a target can or cannot be installed on this machine from this host.
#[derive(Debug, Clone)]
pub struct Assessment {
    pub status: SupportStatus,
    pub live_host: bool,
    pub missing_host: Vec<HostCapability>,
    pub findings: Vec<Finding>,
}

impl Assessment {
    pub fn verdict(&self) -> Verdict {
        let blocked = !self.missing_host.is_empty()
            || self.findings.iter().any(|f| matches!(f, Finding::Blocker(_)));
        match () {
            _ if blocked => Verdict::Blocked,
            _ if !self.live_host => Verdict::NeedsLiveEnvironment,
            _ if !self.status.installable_in_stable() => {
                Verdict::AwaitingCertification(self.status)
            }
            _ => Verdict::Ready,
        }
    }

    pub fn installable(&self) -> bool {
        self.verdict() == Verdict::Ready
    }
}

/// Judge one target against this host and machine. Pure: no I/O.
pub fn assess(
    distro: &Distro,
    spec: &AdapterSpec,
    host: &HostEnvironment,
    hw: &HardwareSnapshot,
) -> Assessment {
    let findings = cpu(distro, hw)
        .into_iter()
        .chain(firmware(hw))
        .chain(device_editions(distro, hw))
        .chain(image_variant(distro, hw))
        .collect();
    Assessment {
        status: distro.status,
        live_host: host.kind.is_live(),
        missing_host: host.missing(spec.host_needs()),
        findings,
    }
}

fn cpu(distro: &Distro, hw: &HardwareSnapshot) -> Vec<Finding> {
    let cpu = match &hw.cpu {
        Fact::Known(cpu) => cpu,
        Fact::Unknown { reason } => return vec![Finding::Unknown(format!("CPU: {reason}"))],
    };
    let architecture = (cpu.architecture != Architecture::X86_64).then(|| {
        Finding::Blocker(format!("{} CPUs are outside the initial x86-64 scope", cpu.architecture))
    });
    let baseline = distro.cpu_baseline.and_then(|baseline| match cpu.x86_level {
        Some(level) if level < baseline => {
            Some(Finding::Blocker(format!("needs a {baseline} CPU; this one is {level}")))
        }
        Some(_) => None,
        None => Some(Finding::Unknown(format!("CPU level unknown; {baseline} required"))),
    });
    architecture.into_iter().chain(baseline).collect()
}

fn firmware(hw: &HardwareSnapshot) -> Option<Finding> {
    match &hw.firmware {
        Fact::Known(FirmwareMode::Uefi { .. }) => None,
        Fact::Known(FirmwareMode::Bios) => {
            Some(Finding::Blocker("legacy BIOS boot is outside the initial UEFI scope".into()))
        }
        Fact::Unknown { reason } => Some(Finding::Unknown(format!("firmware: {reason}"))),
    }
}

fn device_editions<'a>(
    distro: &'a Distro,
    hw: &HardwareSnapshot,
) -> impl Iterator<Item = Finding> + 'a {
    let device = hw.device_class.known().copied();
    distro
        .editions
        .iter()
        .filter(move |e| e.device.is_some() && e.device == device)
        .map(move |e| Finding::Recommend(format!("{} matches this {}", e.name, e.device.unwrap())))
}

fn image_variant(distro: &Distro, hw: &HardwareSnapshot) -> Option<Finding> {
    distro.variants.iter().any(|v| v == "NVIDIA").then(|| {
        Finding::Recommend(if hw.gpus.iter().any(|g| g.vendor == GpuVendor::Nvidia) {
            "NVIDIA image variant: an NVIDIA GPU is present".into()
        } else {
            "Standard image variant: no NVIDIA GPU detected".into()
        })
    })
}

/// Findings about the machine itself, the same whichever target is chosen.
pub fn machine_findings(hw: &HardwareSnapshot) -> Vec<Finding> {
    let microcode = hw.cpu.known().and_then(|cpu| cpu.microcode()).map(|vendor| {
        let vendor = match vendor {
            Microcode::Intel => "Intel",
            Microcode::Amd => "AMD",
        };
        Finding::Recommend(format!("{vendor} CPU microcode will be installed"))
    });
    let hybrid = hw.hybrid_graphics().then(|| {
        let gpus: Vec<_> = hw.gpus.iter().map(|g| g.vendor.to_string()).collect();
        Finding::Recommend(format!("Hybrid graphics ({}): drivers for both", gpus.join(" + ")))
    });
    let wifi = hw.network.iter().any(|n| n.wireless);
    let no_driver = hw.network.iter().any(|n| n.wireless && n.host_driver.is_none());
    let wifi = match (wifi, no_driver) {
        (true, true) => {
            Some(Finding::Unknown("Wi-Fi adapter has no driver in this session".into()))
        }
        (false, _) if hw.network.is_empty() => {
            Some(Finding::Unknown("no network adapter found; online install needs one".into()))
        }
        _ => None,
    };
    let others = hw.disks.iter().filter(|d| d.usage != DiskUsage::InstallationMedium).count();
    let disks = (hw.candidate_disks().next().is_none()).then(|| {
        Finding::Blocker(match others {
            0 => "no disk to install to: only the installer's own medium was found".into(),
            _ => "no disk is free to install to; every disk is in use here".into(),
        })
    });
    microcode.into_iter().chain(hybrid).chain(wifi).chain(disks).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use HostCapability::*;
    use borea_catalog::Catalog;
    use borea_core::{DeviceClass, X86Level};
    use borea_hardware::{Cpu, Gpu};
    use borea_host::{HostKind, OsIdentity};

    fn host(kind: HostKind, os_id: &str) -> HostEnvironment {
        HostEnvironment {
            kind,
            os: Some(OsIdentity { id: os_id.into(), pretty_name: os_id.into() }),
            capabilities: [
                EfiVariables,
                MountNamespaces,
                LoopDevices,
                KernelVfat,
                KernelExt4,
                KernelBtrfs,
                KernelSquashfs,
                KernelOverlay,
                KernelErofs,
                SystemBus,
                NetworkManager,
            ]
            .into(),
        }
    }

    fn live() -> HostEnvironment {
        host(HostKind::LiveSession, "arch")
    }

    fn machine(level: X86Level, gpu: GpuVendor, class: DeviceClass) -> HardwareSnapshot {
        HardwareSnapshot {
            cpu: Fact::Known(Cpu {
                architecture: Architecture::X86_64,
                model: None,
                vendor: None,
                x86_level: Some(level),
                logical_cores: 8,
            }),
            memory_bytes: Fact::Known(16 << 30),
            firmware: Fact::Known(FirmwareMode::Uefi { bits: Some(64) }),
            secure_boot: Fact::Known(false),
            setup_mode: Fact::Known(false),
            device_class: Fact::Known(class),
            gpus: vec![Gpu {
                pci_slot: "0000:01:00.0".into(),
                vendor: gpu,
                device_id: 0,
                host_driver: None,
                boot_display: true,
            }],
            displays: vec![],
            disks: vec![],
            network: vec![],
            bluetooth: vec![],
            inputs: vec![],
            battery: false,
        }
    }

    fn desktop() -> HardwareSnapshot {
        machine(X86Level::V3, GpuVendor::Amd, DeviceClass::Desktop)
    }

    fn judge(id: &str, host: &HostEnvironment, hw: &HardwareSnapshot) -> Assessment {
        let catalog = Catalog::bundled().unwrap();
        assess(catalog.get(id).unwrap(), adapter(id).unwrap(), host, hw)
    }

    fn recommendations(a: &Assessment) -> Vec<&str> {
        a.findings
            .iter()
            .filter_map(|f| match f {
                Finding::Recommend(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn every_catalog_entry_has_an_adapter_and_vice_versa() {
        let catalog = Catalog::bundled().unwrap();
        for distro in &catalog.distros {
            assert!(adapter(distro.id.as_str()).is_some(), "no adapter for {}", distro.id);
        }
        for spec in ADAPTERS {
            assert!(catalog.get(spec.distro).is_some(), "adapter {} not in catalog", spec.distro);
        }
    }

    #[test]
    fn host_distribution_never_restricts_targets() {
        let (fedora, arch) =
            (host(HostKind::LiveSession, "fedora"), host(HostKind::LiveSession, "arch"));
        for spec in ADAPTERS {
            let a = judge(spec.distro, &fedora, &desktop());
            let b = judge(spec.distro, &arch, &desktop());
            assert_eq!(a.verdict(), Verdict::AwaitingCertification(SupportStatus::Planned));
            assert_eq!((a.verdict(), a.findings), (b.verdict(), b.findings), "{}", spec.distro);
        }
    }

    #[test]
    fn missing_kernel_support_blocks_only_targets_that_need_it() {
        let mut no_btrfs = live();
        no_btrfs.capabilities.remove(&KernelBtrfs);
        let fedora = judge("fedora", &no_btrfs, &desktop());
        assert_eq!(fedora.missing_host, [KernelBtrfs]);
        assert_eq!(fedora.verdict(), Verdict::Blocked);
        assert!(judge("arch", &no_btrfs, &desktop()).missing_host.is_empty());
    }

    #[test]
    fn installed_systems_point_to_a_live_environment() {
        let installed = host(HostKind::InstalledSystem, "omarchy");
        assert_eq!(judge("arch", &installed, &desktop()).verdict(), Verdict::NeedsLiveEnvironment);
    }

    #[test]
    fn cpu_baseline_and_variant_recommendation() {
        let old = judge(
            "pikaos",
            &live(),
            &machine(X86Level::V2, GpuVendor::Nvidia, DeviceClass::Desktop),
        );
        assert_eq!(old.verdict(), Verdict::Blocked);
        assert!(
            old.findings
                .contains(&Finding::Blocker("needs a x86-64-v3 CPU; this one is x86-64-v2".into()))
        );
        assert_eq!(recommendations(&old), ["NVIDIA image variant: an NVIDIA GPU is present"]);

        let new = judge("pikaos", &live(), &desktop());
        assert_ne!(new.verdict(), Verdict::Blocked);
        assert_eq!(recommendations(&new), ["Standard image variant: no NVIDIA GPU detected"]);
    }

    #[test]
    fn handheld_editions_are_recommended_on_handhelds() {
        let hw = machine(X86Level::V3, GpuVendor::Amd, DeviceClass::Handheld);
        let cachy = judge("cachyos", &live(), &hw);
        assert_eq!(recommendations(&cachy), ["KDE Plasma · Handheld matches this handheld"]);
        assert!(recommendations(&judge("cachyos", &live(), &desktop())).is_empty());
    }

    #[test]
    fn planned_targets_are_never_installable() {
        let hw = machine(X86Level::V4, GpuVendor::Amd, DeviceClass::Desktop);
        assert!(ADAPTERS.iter().all(|spec| !judge(spec.distro, &live(), &hw).installable()));
    }
}
