#![forbid(unsafe_code)]
//! Drivers are never a question for the user. [`plan_drivers`] turns the scanned machine
//! ([`crate::hardware`]) into the packages, sources, services and boot options each distro needs.
//!
//! NVIDIA, as of late 2026:
//! * **Turing (GTX 16 / RTX 20) and newer** use NVIDIA's open kernel modules, the current branch.
//!   Debian 13 ships 550, which predates Blackwell (RTX 50): those cards fall back to nouveau there.
//! * **Maxwell, Pascal, Volta** (GTX 750–10xx): dropped by driver 590. Arch → `nvidia-580xx-dkms`
//!   (AUR), Fedora → RPM Fusion `akmod-nvidia-580xx`, openSUSE → G06, Debian's 550 still covers them.
//! * **Kepler and older**: no maintained NVIDIA driver; nouveau.
//!
//! Native package names passed the 2026-10-27 upstream inventory audits.
//! Hardware activation and signing evidence is tracked in backend/docs/VERIFIED.md.

use crate::hardware::{AmdGen, Cpu, Family, Hardware, IntelGen, MuxMode, NvGen};
use crate::source::{Distro, Source};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NvDriver {
    /// NVIDIA's open kernel modules, current branch.
    Open,
    /// NVIDIA's proprietary modules on the last branch that supports the card.
    Legacy,
    /// The in-kernel open-source driver (Mesa NVK for Vulkan).
    Nouveau,
}

pub fn nvidia_driver(generation: NvGen, d: Distro) -> NvDriver {
    match generation {
        NvGen::Blackwell if d == Distro::Debian => NvDriver::Nouveau,
        NvGen::Turing | NvGen::Ampere | NvGen::Hopper | NvGen::Ada | NvGen::Blackwell => {
            NvDriver::Open
        }
        NvGen::Maxwell | NvGen::Pascal | NvGen::Volta => NvDriver::Legacy,
        NvGen::Kepler | NvGen::Older => NvDriver::Nouveau,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pkg {
    pub source: Source,
    pub name: &'static str,
}
pub fn debian_gaming_compat(plan: &DriverPlan) -> Vec<Pkg> {
    if plan.packages.iter().any(|package| {
        package.name == "nvidia-driver" && package.source == Source::Repo("debian.non-free")
    }) {
        vec![Pkg { source: Source::Repo("debian.non-free"), name: "nvidia-driver-libs:i386" }]
    } else {
        Vec::new()
    }
}

/// One line for the installer to show: what was found, and what Borea does about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub what: String,
    pub how: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DriverPlan {
    pub found: Vec<Found>,
    pub packages: Vec<Pkg>,
    /// Sources the drivers need; the installer switches these on and keeps them on.
    pub sources: Vec<Source>,
    pub services: Vec<&'static str>,
    pub kernel_args: Vec<&'static str>,
    pub modprobe: Vec<&'static str>,
    /// Out-of-tree modules are self-signed: on first restart the user approves the key (MOK).
    pub mok: bool,
    /// Something the user should know before installing (e.g. the card is better served elsewhere).
    pub warning: Option<String>,
}

impl DriverPlan {
    fn add(&mut self, source: Source, names: &[&'static str]) {
        for &name in names {
            if !self.packages.iter().any(|p| p.name == name) {
                self.packages.push(Pkg { source, name });
            }
        }
        if !self.sources.contains(&source) {
            self.sources.push(source);
        }
    }

    fn say(&mut self, what: impl Into<String>, how: impl Into<String>) {
        self.found.push(Found { what: what.into(), how: how.into() });
    }
}

const fn r(id: &'static str) -> Source {
    Source::Repo(id)
}

pub fn plan_drivers(hw: &Hardware, d: Distro) -> DriverPlan {
    use Distro::*;
    let mut p = DriverPlan::default();
    let laptop = hw.is_laptop();

    let base = match d {
        Arch => r("arch.core-extra"),
        Debian => r("debian.main"),
        Fedora => r("fedora.fedora"),
        OpenSuse => r("opensuse.oss"),
    };
    let fw = if d == Debian { r("debian.non-free-firmware") } else { base };
    let headers = || match d {
        Arch => ["linux-headers"],
        Debian => ["linux-headers-amd64"],
        _ => [""],
    };

    p.say(hw.display_name(), if laptop { "laptop" } else { "desktop" });

    // ---- graphics
    let mut mesa = false;
    // Discrete card first: it's the one people recognise ("RTX 3060").
    let mut gpus: Vec<_> = hw.gpus.iter().collect();
    gpus.sort_by_key(|g| !g.is_discrete(hw.gpus.len()));
    for g in gpus {
        match g.family {
            Family::Nvidia(generation) => {
                let drv = nvidia_driver(generation, d);
                match (d, drv) {
                    (Arch, NvDriver::Open) => {
                        p.add(base, &["nvidia-open", "nvidia-utils", "nvidia-settings"]);
                        p.add(r("arch.multilib"), &["lib32-nvidia-utils"]);
                    }
                    (Arch, NvDriver::Legacy) => {
                        p.add(base, &headers());
                        p.add(
                            Source::Aur,
                            &[
                                "nvidia-580xx-dkms",
                                "nvidia-580xx-utils",
                                "lib32-nvidia-580xx-utils",
                            ],
                        );
                    }
                    (Debian, NvDriver::Open) => {
                        p.add(base, &headers());
                        p.add(r("debian.non-free"), &["nvidia-driver"]);
                        p.add(r("debian.contrib"), &["nvidia-open-kernel-dkms"]);
                        p.add(fw, &["firmware-misc-nonfree"]);
                        p.kernel_args.push("nvidia-drm.modeset=1");
                        p.mok |= hw.secure_boot;
                    }
                    (Debian, NvDriver::Legacy) => {
                        p.add(base, &headers());
                        p.add(r("debian.non-free"), &["nvidia-driver", "nvidia-kernel-dkms"]);
                        p.kernel_args.push("nvidia-drm.modeset=1");
                        p.mok |= hw.secure_boot;
                    }
                    (Fedora, NvDriver::Open) => {
                        p.add(
                            r("fedora.rpmfusion-nonfree"),
                            &["akmod-nvidia", "xorg-x11-drv-nvidia-cuda"],
                        );
                        if laptop {
                            p.add(r("fedora.rpmfusion-nonfree"), &["xorg-x11-drv-nvidia-power"]);
                        }
                        p.mok |= hw.secure_boot;
                    }
                    (Fedora, NvDriver::Legacy) => {
                        p.add(
                            r("fedora.rpmfusion-nonfree"),
                            &[
                                "akmod-nvidia-580xx",
                                "xorg-x11-drv-nvidia-580xx",
                                "xorg-x11-drv-nvidia-580xx-cuda",
                            ],
                        );
                        p.mok |= hw.secure_boot;
                    }
                    (OpenSuse, NvDriver::Open) => {
                        // Kernel modules signed by openSUSE (no key to approve); userspace from NVIDIA's repo.
                        p.add(base, &["nvidia-open-driver-G07-signed-kmp-meta"]);
                        p.add(
                            r("opensuse.nvidia"),
                            &["nvidia-video-G07", "nvidia-gl-G07", "nvidia-compute-utils-G07"],
                        );
                    }
                    (OpenSuse, NvDriver::Legacy) => {
                        p.add(
                            r("opensuse.nvidia"),
                            &[
                                "nvidia-driver-G06-kmp-meta",
                                "nvidia-video-G06",
                                "nvidia-gl-G06",
                                "nvidia-compute-utils-G06",
                            ],
                        );
                        p.mok |= hw.secure_boot;
                    }
                    (_, NvDriver::Nouveau) => mesa = true,
                }
                if drv != NvDriver::Nouveau {
                    p.modprobe.push("options nvidia NVreg_PreserveVideoMemoryAllocations=1");
                    if laptop {
                        p.services.extend([
                            "nvidia-suspend.service",
                            "nvidia-resume.service",
                            "nvidia-hibernate.service",
                        ]);
                        if generation >= NvGen::Ampere {
                            p.services.push("nvidia-powerd.service"); // Dynamic Boost
                        }
                    }
                }
                let how = match drv {
                    NvDriver::Open => "NVIDIA driver (open modules)".to_string(),
                    NvDriver::Legacy if d == Debian => "NVIDIA 550 driver".to_string(),
                    NvDriver::Legacy => "NVIDIA 580 legacy driver".to_string(),
                    NvDriver::Nouveau => "open-source nouveau".to_string(),
                };
                p.say(g.name.clone(), how);
                if generation == NvGen::Blackwell && d == Debian {
                    p.warning = Some(format!(
                        "Debian 13's NVIDIA driver is too old for the {}: it runs on nouveau, without full speed. Arch, Fedora and openSUSE support it fully.",
                        g.name
                    ));
                } else if drv == NvDriver::Nouveau {
                    p.warning = Some(format!(
                        "NVIDIA no longer supports the {} ({}): it runs on nouveau, fine for the desktop but slow for games.",
                        g.name,
                        generation.label()
                    ));
                }
            }
            Family::Amd(generation) => {
                mesa = true;
                match (d, generation) {
                    (Arch, AmdGen::TeraScale) => {}
                    (Arch, _) => {
                        p.add(base, &["vulkan-radeon"]);
                        p.add(r("arch.multilib"), &["lib32-vulkan-radeon"]);
                    }
                    (Debian, _) => {
                        p.add(base, &["mesa-vulkan-drivers", "mesa-va-drivers"]);
                        p.add(fw, &["firmware-amd-graphics"]);
                    }
                    (Fedora, _) => {
                        p.add(base, &["mesa-vulkan-drivers"]);
                        p.add(r("fedora.rpmfusion-free"), &["mesa-va-drivers-freeworld"]);
                    }
                    (OpenSuse, _) => p.add(base, &["libvulkan_radeon"]),
                }
                // Debian 13's kernel (6.12) still hands GCN 1/2 to the old radeon driver.
                if d == Debian {
                    match generation {
                        AmdGen::Gcn1 => {
                            p.kernel_args.extend(["radeon.si_support=0", "amdgpu.si_support=1"])
                        }
                        AmdGen::Gcn2 => {
                            p.kernel_args.extend(["radeon.cik_support=0", "amdgpu.cik_support=1"])
                        }
                        _ => {}
                    }
                }
                p.say(
                    g.name.clone(),
                    if generation == AmdGen::TeraScale {
                        "Mesa · radeon"
                    } else {
                        "Mesa · amdgpu"
                    },
                );
            }
            Family::Intel(generation) => {
                mesa = true;
                let va_new = generation != IntelGen::Legacy;
                match d {
                    Arch => {
                        p.add(
                            base,
                            &[
                                "vulkan-intel",
                                if va_new { "intel-media-driver" } else { "libva-intel-driver" },
                            ],
                        );
                        p.add(r("arch.multilib"), &["lib32-vulkan-intel"]);
                    }
                    Debian => {
                        p.add(base, &["mesa-vulkan-drivers"]);
                        if va_new {
                            p.add(r("debian.non-free"), &["intel-media-va-driver-non-free"]);
                        } else {
                            p.add(base, &["i965-va-driver"]);
                        }
                        p.add(fw, &["firmware-intel-graphics"]); // Confirmed in Debian 13 non-free-firmware.
                    }
                    Fedora => p.add(
                        base,
                        &[
                            "mesa-vulkan-drivers",
                            if va_new { "libva-intel-media-driver" } else { "libva-intel-driver" },
                        ],
                    ),
                    OpenSuse => p.add(
                        base,
                        &[
                            "libvulkan_intel",
                            if va_new { "intel-media-driver" } else { "intel-vaapi-driver" },
                        ],
                    ),
                }
                p.say(
                    g.name.clone(),
                    if generation == IntelGen::Arc { "Mesa · xe" } else { "Mesa" },
                );
            }
            Family::Other => p.say(g.name.clone(), "kernel default"),
        }
    }
    if mesa {
        match d {
            Arch => {
                p.add(base, &["mesa"]);
                p.add(r("arch.multilib"), &["lib32-mesa"]);
            }
            Debian => p.add(base, &["mesa-vulkan-drivers"]),
            Fedora => p.add(base, &["mesa-dri-drivers", "mesa-vulkan-drivers"]),
            OpenSuse => p.add(base, &["Mesa-dri"]),
        }
        if hw.nvidia().is_some_and(|g| nvidia_driver(g, d) == NvDriver::Nouveau) {
            match d {
                Arch => p.add(base, &["vulkan-nouveau"]),
                Debian => p.add(fw, &["firmware-misc-nonfree"]),
                OpenSuse => p.add(base, &["libvulkan_nouveau"]),
                Fedora => {}
            }
        }
    }

    if hw.gpus.is_empty() {
        p.say("graphics", "none found · basic display");
    }

    // ---- laptops with two GPUs: render on the dGPU on demand, menus offer "Launch using discrete GPU".
    if hw.hybrid() {
        p.add(base, &["switcheroo-control"]);
        p.services.push("switcheroo-control.service");
        if d == Arch && hw.nvidia().is_some_and(|g| nvidia_driver(g, d) != NvDriver::Nouveau) {
            p.add(base, &["nvidia-prime"]);
        }
    }
    if let Some(m) = &hw.mux {
        let now = if m.mode == MuxMode::Hybrid { "hybrid now" } else { "dGPU only now" };
        let how = if m.control.is_some() {
            format!("{now} · both modes set up, switch any time (restart)")
        } else {
            format!("{now} · both modes set up (switch in firmware)")
        };
        p.say("MUX switch", how);
    }
    if laptop {
        p.add(base, &[if d == Fedora { "tuned-ppd" } else { "power-profiles-daemon" }]);
        if d == Fedora {
            p.services.extend(["tuned.service", "tuned-ppd.service"]);
        } else {
            p.services.push("power-profiles-daemon.service");
        } // Laptop service activation remains a hardware verification gate.
    }

    // ---- CPU, audio, wireless, fingerprint
    match (d, hw.cpu) {
        (Arch, Cpu::Intel) => p.add(base, &["intel-ucode"]),
        (Arch, Cpu::Amd) => p.add(base, &["amd-ucode"]),
        (Debian, Cpu::Intel) => p.add(fw, &["intel-microcode"]),
        (Debian, Cpu::Amd) => p.add(fw, &["amd64-microcode"]),
        (Fedora, Cpu::Intel) => p.add(base, &["microcode_ctl"]),
        (Fedora, Cpu::Amd) => p.add(base, &["amd-ucode-firmware"]),
        (OpenSuse, Cpu::Intel) => p.add(base, &["ucode-intel"]),
        (OpenSuse, Cpu::Amd) => p.add(base, &["ucode-amd"]),
        (_, Cpu::Other) => {}
    }
    let mut extras = vec![match hw.cpu {
        Cpu::Intel => "Intel microcode",
        Cpu::Amd => "AMD microcode",
        Cpu::Other => "",
    }];
    if hw.sof_audio {
        match d {
            Arch => p.add(base, &["sof-firmware"]),
            Debian => p.add(fw, &["firmware-sof-signed"]),
            Fedora => p.add(base, &["alsa-sof-firmware"]),
            OpenSuse => p.add(base, &["sof-firmware"]),
        }
        extras.push("speakers & mics");
    }
    if hw.broadcom_wl {
        match d {
            Arch => {
                p.add(base, &headers());
                p.add(base, &["broadcom-wl-dkms"]);
            }
            Debian => {
                p.add(base, &headers());
                p.add(r("debian.non-free"), &["broadcom-sta-dkms"]);
                p.mok |= hw.secure_boot;
            }
            Fedora => {
                p.add(r("fedora.rpmfusion-nonfree"), &["akmod-wl"]);
                p.mok |= hw.secure_boot;
            }
            OpenSuse => p.add(r("opensuse.packman"), &["broadcom-wl", "broadcom-wl-kmp-default"]),
        }
        extras.push("Broadcom Wi-Fi");
    }
    if let Some(f) = &hw.fingerprint {
        match d {
            Debian => p.add(base, &["fprintd", "libpam-fprintd"]),
            Arch => p.add(base, &["fprintd"]),
            _ => p.add(base, &["fprintd", "fprintd-pam"]),
        }
        extras.push(if f.is_empty() { "fingerprint" } else { "fingerprint reader" });
    }
    extras.retain(|e| !e.is_empty());
    if !extras.is_empty() {
        p.say("also", extras.join(" · "));
    }
    p.packages.retain(|x| !x.name.is_empty());

    // Arch doesn't sign anything: Secure Boot has to be off, or keys enrolled with sbctl.
    if d == Arch && hw.secure_boot && p.warning.is_none() {
        p.warning = Some("Secure Boot is on. Arch isn't signed for it: Borea sets up sbctl, which needs Secure Boot in Setup Mode (firmware settings).".into());
    }
    p
}
