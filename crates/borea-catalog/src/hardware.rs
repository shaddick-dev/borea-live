#![forbid(unsafe_code)]
//! What machine Borea is running on, read from sysfs, DMI and /proc on the live ISO.
//!
//! [`scan`] takes a root path so the same code runs against `/` on real hardware and against a
//! fixture tree in tests. Nothing here decides drivers; that is [`crate::drivers`].
//!
//! GPU generations are worked out from PCI device-ID ranges. That is accurate for the families
//! listed below; a release build should prefer NVIDIA's `supported-gpus.json` (shipped with the
//! driver, it names the exact legacy branch per device) and fall back to these ranges.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const NVIDIA: u16 = 0x10de;
pub const AMD: u16 = 0x1002;
pub const INTEL: u16 = 0x8086;
pub const BROADCOM: u16 = 0x14e4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NvGen {
    /// Fermi and earlier.
    Older,
    Kepler,
    Maxwell,
    Pascal,
    Volta,
    /// GTX 16xx and RTX 20xx.
    Turing,
    /// RTX 30xx.
    Ampere,
    Hopper,
    /// RTX 40xx.
    Ada,
    /// RTX 50xx.
    Blackwell,
}

impl NvGen {
    pub fn from_device(id: u16) -> NvGen {
        use NvGen::*;
        match id {
            0x2900..=0x2fff => Blackwell,
            0x2680..=0x28ff => Ada,
            0x2300..=0x233f => Hopper,
            0x2080..=0x20ff | 0x2200..=0x25ff => Ampere,
            0x1e00..=0x1fff | 0x2180..=0x21ff => Turing,
            0x1d80..=0x1dbf => Volta,
            0x15f0..=0x15ff | 0x1b00..=0x1d7f => Pascal,
            0x1340..=0x17ff => Maxwell,
            0x0fc0..=0x12ff => Kepler,
            _ => Older,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            NvGen::Older => "pre-Kepler",
            NvGen::Kepler => "Kepler",
            NvGen::Maxwell => "Maxwell",
            NvGen::Pascal => "Pascal",
            NvGen::Volta => "Volta",
            NvGen::Turing => "Turing",
            NvGen::Ampere => "Ampere",
            NvGen::Hopper => "Hopper",
            NvGen::Ada => "Ada Lovelace",
            NvGen::Blackwell => "Blackwell",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmdGen {
    /// HD 2000–6000 and Trinity/Richland APUs: `radeon` kernel driver, no Vulkan.
    TeraScale,
    /// HD 7000 (Southern Islands): `amdgpu` needs `si_support` on kernels before 6.19.
    Gcn1,
    /// R7/R9 200, Kaveri/Kabini (Sea Islands): `amdgpu` needs `cik_support` before 6.19.
    Gcn2,
    /// GCN 3+ and every RDNA: `amdgpu`, nothing to configure.
    Modern,
}

impl AmdGen {
    pub fn from_device(id: u16) -> AmdGen {
        match id {
            0x6600..=0x663f | 0x6660..=0x667f | 0x6780..=0x679f | 0x6800..=0x683f => AmdGen::Gcn1,
            0x6640..=0x665f | 0x67a0..=0x67bf | 0x1304..=0x131f | 0x9830..=0x985f => AmdGen::Gcn2,
            0x6700..=0x677f
            | 0x6840..=0x68ff
            | 0x9400..=0x972f
            | 0x9800..=0x982f
            | 0x9900..=0x99ff => AmdGen::TeraScale,
            _ => AmdGen::Modern,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntelGen {
    /// Before Broadwell: VA-API through the old i965 driver.
    Legacy,
    /// Broadwell and newer integrated graphics.
    Modern,
    /// Arc discrete cards (Alchemist, Battlemage).
    Arc,
}

impl IntelGen {
    pub fn from_device(id: u16) -> IntelGen {
        match id {
            0x5690..=0x56ff | 0xe202..=0xe21f => IntelGen::Arc,
            0x0000..=0x15ff => IntelGen::Legacy,
            _ => IntelGen::Modern,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Nvidia(NvGen),
    Amd(AmdGen),
    Intel(IntelGen),
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gpu {
    pub slot: String,
    pub vendor: u16,
    pub device: u16,
    pub name: String,
    pub family: Family,
    /// The firmware used this GPU to show the boot screen.
    pub boot_vga: bool,
    /// The built-in screen (eDP/LVDS) is wired to this GPU right now.
    pub drives_panel: bool,
}

impl Gpu {
    pub fn new(slot: &str, vendor: u16, device: u16, name: &str) -> Gpu {
        let family = match vendor {
            NVIDIA => Family::Nvidia(NvGen::from_device(device)),
            AMD => Family::Amd(AmdGen::from_device(device)),
            INTEL => Family::Intel(IntelGen::from_device(device)),
            _ => Family::Other,
        };
        Gpu {
            slot: slot.into(),
            vendor,
            device,
            name: name.into(),
            family,
            boot_vga: false,
            drives_panel: false,
        }
    }

    /// NVIDIA and Arc are always separate cards; an AMD GPU counts as discrete when it sits
    /// beside another GPU without being the boot display (the APU is the boot display).
    pub fn is_discrete(&self, gpu_count: usize) -> bool {
        match self.family {
            Family::Nvidia(_) | Family::Intel(IntelGen::Arc) => true,
            Family::Amd(_) => gpu_count > 1 && !self.boot_vga,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chassis {
    Desktop,
    Laptop,
    Other,
}

impl Chassis {
    /// SMBIOS chassis type codes.
    pub fn from_code(code: u8) -> Chassis {
        match code {
            8 | 9 | 10 | 14 | 30 | 31 | 32 => Chassis::Laptop,
            3..=7 | 13 | 15 | 16 | 35 | 36 => Chassis::Desktop,
            _ => Chassis::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cpu {
    Intel,
    Amd,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MuxMode {
    /// iGPU drives the screen, the dGPU renders on demand (Optimus / PRIME offload).
    Hybrid,
    /// The screen is wired straight to the dGPU; the iGPU is idle or off.
    Discrete,
}

/// A hardware GPU switch, as found on many gaming laptops (ASUS TUF/ROG, Legion, Omen…).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mux {
    pub mode: MuxMode,
    /// sysfs file that flips it (needs a reboot), when the kernel exposes one.
    pub control: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hardware {
    pub maker: String,
    pub model: String,
    /// DMI product_version: Lenovo keeps the marketing name here ("ThinkPad X1 Carbon Gen 11").
    pub version: String,
    pub board: String,
    pub chassis: Chassis,
    pub battery: bool,
    pub cpu: Cpu,
    /// The x86-64 microarchitecture level, 1 to 4, from the CPU's flags.
    pub x86_level: u8,
    pub gpus: Vec<Gpu>,
    pub mux: Option<Mux>,
    pub secure_boot: bool,
    pub setup_mode: Option<bool>,
    /// Wi-Fi/Bluetooth controllers that need a non-default driver (Broadcom `wl`).
    pub broadcom_wl: bool,
    /// Intel audio DSP (laptop microphones and speakers need Sound Open Firmware).
    pub sof_audio: bool,
    /// USB fingerprint reader from a vendor libfprint supports.
    pub fingerprint: Option<String>,
}

impl Hardware {
    pub fn is_laptop(&self) -> bool {
        self.chassis == Chassis::Laptop || (self.chassis == Chassis::Other && self.battery)
    }

    pub fn discrete(&self) -> Option<&Gpu> {
        self.gpus.iter().find(|g| g.is_discrete(self.gpus.len()))
    }

    pub fn integrated(&self) -> Option<&Gpu> {
        self.gpus.iter().find(|g| !g.is_discrete(self.gpus.len()))
    }

    /// A laptop with an iGPU and a dGPU.
    pub fn hybrid(&self) -> bool {
        self.is_laptop() && self.discrete().is_some() && self.integrated().is_some()
    }

    pub fn nvidia(&self) -> Option<NvGen> {
        self.gpus.iter().find_map(|g| match g.family {
            Family::Nvidia(n) => Some(n),
            _ => None,
        })
    }

    /// "ASUS TUF Gaming F15" from DMI's "ASUS TUF Gaming F15 FX507ZM_FX507ZM" + board "FX507ZM".
    pub fn display_name(&self) -> String {
        let maker = match self.maker.as_str() {
            m if m.starts_with("ASUSTeK") => "ASUS",
            m if m.starts_with("LENOVO") => "Lenovo",
            m if m.starts_with("Hewlett-Packard") || m == "HP" => "HP",
            m if m.starts_with("Micro-Star") => "MSI",
            m if m.starts_with("Dell") => "Dell",
            m if m.starts_with("Acer") => "Acer",
            m if m.starts_with("Gigabyte") => "Gigabyte",
            m => m,
        };
        if maker.is_empty() && self.model.is_empty() && self.board.is_empty() {
            return "This computer".into(); // no DMI (some VMs and boards)
        }
        if maker == "Lenovo" && !self.version.is_empty() && self.version != "None" {
            return format!("Lenovo {}", self.version);
        }
        let code_like = !self.model.contains(' ') && self.board.contains(' ');
        if code_like {
            return format!("{maker} {}", self.board);
        }
        let model: Vec<&str> = self
            .model
            .split_whitespace()
            .filter(|w| !w.contains(&*self.board) || self.board.is_empty())
            .collect();
        let model = model.join(" ");
        if model.is_empty()
            || model.eq_ignore_ascii_case("To Be Filled By O.E.M.")
            || model.eq_ignore_ascii_case("System Product Name")
        {
            return if self.board.is_empty() {
                maker.to_string()
            } else {
                format!("{maker} {}", self.board)
            };
        }
        if model.to_lowercase().starts_with(&maker.to_lowercase()) {
            model
        } else {
            format!("{maker} {model}")
        }
    }
}

// ---------------------------------------------------------------- scanning

fn read(root: &Path, p: &str) -> Option<String> {
    fs::read_to_string(root.join(p.trim_start_matches('/'))).ok().map(|s| s.trim().to_string())
}

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

fn dirs(p: PathBuf) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(p)
        .map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// The x86-64 level (1 to 4) the first CPU's flags meet, as psABI defines them.
pub fn x86_level(cpuinfo: &str) -> u8 {
    let flags = cpuinfo
        .lines()
        .find_map(|l| l.strip_prefix("flags").and_then(|r| r.split_once(':')).map(|(_, f)| f))
        .unwrap_or_default()
        .split_whitespace()
        .collect::<std::collections::BTreeSet<_>>();
    let levels: [&[&str]; 3] = [
        &["cx16", "lahf_lm", "popcnt", "sse4_1", "sse4_2", "ssse3"],
        &["avx", "avx2", "bmi1", "bmi2", "f16c", "fma", "abm", "movbe", "xsave"],
        &["avx512f", "avx512bw", "avx512cd", "avx512dq", "avx512vl"],
    ];
    1 + levels.iter().take_while(|needed| needed.iter().all(|f| flags.contains(f))).count() as u8
}

/// Read the machine from `root` (`/` on the live ISO). `names` looks up PCI names (pci.ids).
pub fn scan(root: &Path, names: &dyn Fn(u16, u16) -> Option<String>) -> io::Result<Hardware> {
    let dmi = |f: &str| read(root, &format!("sys/class/dmi/id/{f}")).unwrap_or_default();
    let chassis = Chassis::from_code(dmi("chassis_type").parse().unwrap_or(2));
    let battery = dirs(root.join("sys/class/power_supply")).iter().any(|p| {
        fs::read_to_string(p.join("type")).map(|t| t.trim() == "Battery").unwrap_or(false)
    });
    let cpuinfo = read(root, "proc/cpuinfo").unwrap_or_default();
    let cpu = if cpuinfo.contains("GenuineIntel") {
        Cpu::Intel
    } else if cpuinfo.contains("AuthenticAMD") {
        Cpu::Amd
    } else {
        Cpu::Other
    };

    let x86_level = x86_level(&cpuinfo);
    let mut gpus = Vec::new();
    let mut broadcom_wl = false;
    let mut sof_audio = false;
    for dev in dirs(root.join("sys/bus/pci/devices")) {
        let r = |f: &str| fs::read_to_string(dev.join(f)).ok();
        let (Some(class), Some(vendor), Some(device)) = (
            r("class").and_then(|s| hex(&s)),
            r("vendor").and_then(|s| hex(&s)),
            r("device").and_then(|s| hex(&s)),
        ) else {
            continue;
        };
        let (vendor, device) = (vendor as u16, device as u16);
        let slot = dev.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match class >> 8 {
            0x0300 | 0x0302 | 0x0380 => {
                let name = names(vendor, device)
                    .map(|n| tidy_name(vendor, &n))
                    .unwrap_or_else(|| format!("{vendor:04x}:{device:04x}"));
                let mut g = Gpu::new(&slot, vendor, device, &name);
                g.boot_vga = r("boot_vga").map(|s| s.trim() == "1").unwrap_or(false);
                gpus.push(g);
            }
            0x0280 if vendor == BROADCOM && WL_ONLY.contains(&device) => broadcom_wl = true,
            0x0401 if vendor == INTEL => sof_audio = true,
            _ => {}
        }
    }
    // Which GPU owns the built-in panel: /sys/class/drm/cardN-eDP-1 -> cardN/device -> PCI slot.
    for conn in dirs(root.join("sys/class/drm")) {
        let n = conn.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !(n.contains("-eDP-") || n.contains("-LVDS-")) {
            continue;
        }
        if fs::read_to_string(conn.join("status")).map(|s| s.trim() != "connected").unwrap_or(false)
        {
            continue;
        }
        let card = n.split('-').next().unwrap_or_default();
        let slot = read(root, &format!("sys/class/drm/{card}/device/slot")).unwrap_or_default();
        if let Some(g) = gpus.iter_mut().find(|g| g.slot == slot) {
            g.drives_panel = true;
        }
    }

    // GPU MUX: ASUS exposes it (asus-armoury since Linux 6.19, asus-wmi before); 0 = dGPU, 1 = Optimus.
    let mux_files = [
        "sys/class/firmware-attributes/asus-armoury/attributes/gpu_mux_mode/current_value",
        "sys/devices/platform/asus-nb-wmi/gpu_mux_mode",
    ];
    let mut mux = mux_files.iter().find_map(|f| {
        read(root, f).map(|v| Mux {
            mode: if v == "0" { MuxMode::Discrete } else { MuxMode::Hybrid },
            control: Some(format!("/{f}")),
        })
    });
    // Other brands: no kernel switch, but the panel tells us. A laptop whose panel hangs off the
    // dGPU while an iGPU exists has its MUX set to discrete (set in the firmware menu).
    let hw_tmp = Hardware {
        maker: dmi("sys_vendor"),
        model: dmi("product_name"),
        version: dmi("product_version"),
        board: dmi("board_name"),
        chassis,
        battery,
        cpu,
        x86_level,
        gpus,
        mux: None,
        secure_boot: false,
        setup_mode: None,
        broadcom_wl,
        sof_audio,
        fingerprint: None,
    };
    if mux.is_none() && hw_tmp.hybrid() && hw_tmp.discrete().is_some_and(|g| g.drives_panel) {
        mux = Some(Mux { mode: MuxMode::Discrete, control: None });
    }

    let secure_boot = dirs(root.join("sys/firmware/efi/efivars"))
        .iter()
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("SecureBoot-")))
        .and_then(|p| fs::read(p).ok())
        .is_some_and(|b| b.last() == Some(&1));

    let fingerprint = dirs(root.join("sys/bus/usb/devices")).iter().find_map(|d| {
        let v = fs::read_to_string(d.join("idVendor")).ok().and_then(|s| hex(&s))? as u16;
        FPRINT.iter().find(|(id, _)| *id == v).map(|(_, n)| n.to_string())
    });

    let setup_mode = dirs(root.join("sys/firmware/efi/efivars"))
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("SetupMode-")))
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| b.get(4).copied())
        .map(|v| v != 0);
    Ok(Hardware { mux, secure_boot, setup_mode, fingerprint, ..hw_tmp })
}

/// Broadcom chips with no in-kernel driver (b43/brcmfmac don't cover them): need `wl`.
pub const WL_ONLY: &[u16] = &[
    0x4311, 0x4312, 0x4313, 0x4315, 0x4328, 0x4329, 0x432a, 0x432b, 0x432c, 0x432d, 0x4353, 0x4357,
    0x4358, 0x4359, 0x4365, 0x43a0, 0x43b1,
];

/// USB vendors whose fingerprint readers libfprint drives (model support still varies).
pub const FPRINT: &[(u16, &str)] = &[
    (0x06cb, "Synaptics"),
    (0x27c6, "Goodix"),
    (0x138a, "Validity"),
    (0x04f3, "Elan"),
    (0x1c7a, "Egis"),
];

/// pci.ids names are chip codes plus marketing names: "GA106M [GeForce RTX 3060 Mobile / Max-Q]".
/// Keep the marketing name a person recognises: "GeForce RTX 3060 Mobile".
pub fn tidy_name(vendor: u16, raw: &str) -> String {
    let mut n = match (raw.find('['), raw.rfind(']')) {
        (Some(a), Some(b)) if b > a => raw[a + 1..b].to_string(),
        _ => raw.to_string(),
    };
    for variant in [" / Max-Q", " / Mobile", " / Max-Q Design"] {
        if let Some(i) = n.find(variant) {
            n.truncate(i);
        }
    }
    if vendor == INTEL && !n.starts_with("Intel") {
        n = format!("Intel {n}");
    }
    n.trim().to_string()
}

/// Minimal reader for hwdata's `pci.ids` ("vendor\n\tdevice  name").
pub fn pci_ids(text: &str) -> impl Fn(u16, u16) -> Option<String> + '_ {
    move |vendor, device| {
        let mut in_vendor = false;
        for line in text.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if !line.starts_with('\t') {
                in_vendor = hex(line.get(..4)?) == Some(vendor as u32);
                continue;
            }
            if in_vendor && !line.starts_with("\t\t") {
                let l = line.trim_start();
                if hex(l.get(..4)?) == Some(device as u32) {
                    return Some(l[4..].trim().to_string());
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------- sample machines

/// Fixtures only: stand-in machines for tests and the design prototype's "hw" switch.
/// The live ISO never uses these; it runs [`scan`] on the machine it booted on (see [`crate::probe`]).
pub fn samples() -> Vec<(&'static str, Hardware)> {
    let gpu = |slot: &str, v: u16, d: u16, name: &str, boot: bool, panel: bool| {
        let mut g = Gpu::new(slot, v, d, name);
        g.boot_vga = boot;
        g.drives_panel = panel;
        g
    };
    let base = |maker: &str, model: &str, board: &str, chassis: Chassis, cpu: Cpu| Hardware {
        maker: maker.into(),
        model: model.into(),
        version: String::new(),
        board: board.into(),
        chassis,
        battery: chassis == Chassis::Laptop,
        cpu,
        x86_level: 3,
        gpus: vec![],
        mux: None,
        secure_boot: false,
        setup_mode: None,
        broadcom_wl: false,
        sof_audio: false,
        fingerprint: None,
    };
    vec![
        ("tuf-f15", Hardware {
            gpus: vec![
                gpu("0000:00:02.0", INTEL, 0x46a6, "Intel Iris Xe (Alder Lake)", true, true),
                gpu("0000:01:00.0", NVIDIA, 0x2520, "GeForce RTX 3060 Laptop", false, false),
            ],
            mux: Some(Mux { mode: MuxMode::Hybrid, control: Some("/sys/class/firmware-attributes/asus-armoury/attributes/gpu_mux_mode/current_value".into()) }),
            secure_boot: true, sof_audio: true,
            ..base("ASUSTeK COMPUTER INC.", "ASUS TUF Gaming F15 FX507ZM_FX507ZM", "FX507ZM", Chassis::Laptop, Cpu::Intel)
        }),
        ("gtx1080", Hardware {
            gpus: vec![gpu("0000:01:00.0", NVIDIA, 0x1b80, "GeForce GTX 1080", true, false)],
            ..base("Micro-Star International Co., Ltd.", "MS-7C02", "B450 TOMAHAWK MAX", Chassis::Desktop, Cpu::Amd)
        }),
        ("rtx5070", Hardware {
            gpus: vec![gpu("0000:01:00.0", NVIDIA, 0x2f04, "GeForce RTX 5070", true, false)],
            secure_boot: true,
            ..base("ASUSTeK COMPUTER INC.", "System Product Name", "PRIME B760-PLUS", Chassis::Desktop, Cpu::Intel)
        }),
        ("rx7800", Hardware {
            gpus: vec![gpu("0000:03:00.0", AMD, 0x747e, "Radeon RX 7800 XT", true, false)],
            ..base("Gigabyte Technology Co., Ltd.", "B650 AORUS ELITE AX", "B650 AORUS ELITE AX", Chassis::Desktop, Cpu::Amd)
        }),
        ("thinkpad", Hardware {
            gpus: vec![gpu("0000:00:02.0", INTEL, 0xa7a0, "Intel Iris Xe (Raptor Lake)", true, true)],
            secure_boot: true, sof_audio: true, fingerprint: Some("Synaptics".into()), version: "ThinkPad X1 Carbon Gen 11".into(),
            ..base("LENOVO", "21HM0047UK", "21HM0047UK", Chassis::Laptop, Cpu::Intel)
        }),
    ]
}
