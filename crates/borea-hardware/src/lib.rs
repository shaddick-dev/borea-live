//! Read-only hardware discovery.
//!
//! Two layers, kept apart on purpose:
//!
//! - [`parse`]: pure functions from kernel text to facts. No I/O, trivially testable.
//! - [`Scanner`]: reads procfs/sysfs beneath a configurable root and feeds the parsers,
//!   so the same code runs against `/` and against fixture trees.
//!
//! Nothing here mounts, writes, loads modules, or needs root. A probe that cannot
//! answer reports [`Fact::Unknown`]; it never guesses.

pub mod parse;

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use borea_core::{Architecture, DeviceClass, Fact, FirmwareMode, X86Level};

use parse::Mount;

#[derive(Debug, Clone)]
pub struct HardwareSnapshot {
    pub cpu: Fact<Cpu>,
    pub memory_bytes: Fact<u64>,
    pub firmware: Fact<FirmwareMode>,
    pub secure_boot: Fact<bool>,
    /// The firmware accepts new Secure Boot keys without a password (no platform key
    /// is enrolled). Only then can keys be enrolled automatically.
    pub setup_mode: Fact<bool>,
    pub device_class: Fact<DeviceClass>,
    pub gpus: Vec<Gpu>,
    pub displays: Vec<Display>,
    pub disks: Vec<Disk>,
    pub network: Vec<NetworkAdapter>,
    pub bluetooth: Vec<String>,
    pub inputs: Vec<InputDevice>,
    pub battery: bool,
}

impl HardwareSnapshot {
    /// More than one GPU, as on hybrid laptops: the plan must keep drivers for both.
    pub fn hybrid_graphics(&self) -> bool {
        self.gpus.len() > 1
    }

    /// Disks that could be offered as installation targets. Never preselected.
    pub fn candidate_disks(&self) -> impl Iterator<Item = &Disk> {
        self.disks.iter().filter(|d| d.usage == DiskUsage::Free)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cpu {
    pub architecture: Architecture,
    pub model: Option<String>,
    pub vendor: Option<String>,
    /// Highest x86-64 micro-architecture level whose flags are all present.
    pub x86_level: Option<X86Level>,
    pub logical_cores: usize,
}

impl Cpu {
    /// Microcode the target should carry for this CPU vendor.
    pub fn microcode(&self) -> Option<Microcode> {
        match self.vendor.as_deref() {
            Some("GenuineIntel") => Some(Microcode::Intel),
            Some("AuthenticAMD") => Some(Microcode::Amd),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Microcode {
    Intel,
    Amd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Amd,
    Intel,
    Nvidia,
    /// Emulated graphics in a virtual machine, by hypervisor.
    Virtual(&'static str),
    Other(u16),
}

impl GpuVendor {
    pub fn from_pci(id: u16) -> Self {
        match id {
            0x1002 => GpuVendor::Amd,
            0x8086 => GpuVendor::Intel,
            0x10de => GpuVendor::Nvidia,
            0x1234 => GpuVendor::Virtual("QEMU"),
            0x1af4 | 0x1b36 => GpuVendor::Virtual("VirtIO"),
            0x15ad => GpuVendor::Virtual("VMware"),
            0x80ee => GpuVendor::Virtual("VirtualBox"),
            0x1414 => GpuVendor::Virtual("Hyper-V"),
            other => GpuVendor::Other(other),
        }
    }
}

impl fmt::Display for GpuVendor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuVendor::Amd => f.write_str("AMD"),
            GpuVendor::Intel => f.write_str("Intel"),
            GpuVendor::Nvidia => f.write_str("NVIDIA"),
            GpuVendor::Virtual(name) => write!(f, "{name} virtual"),
            GpuVendor::Other(id) => write!(f, "PCI vendor {id:04x}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gpu {
    pub pci_slot: String,
    pub vendor: GpuVendor,
    pub device_id: u16,
    /// Kernel driver bound in the *host*. It says nothing certain about the target's
    /// driver; adapters make that recommendation.
    pub host_driver: Option<String>,
    pub boot_display: bool,
}

impl Gpu {
    /// What to call it on screen: "AMD 73bf", "QEMU virtual graphics", or the PCI
    /// IDs when the vendor is unknown.
    pub fn label(&self) -> String {
        match self.vendor {
            GpuVendor::Virtual(name) => format!("{name} virtual graphics"),
            GpuVendor::Other(vendor) => format!("PCI {vendor:04x}:{:04x}", self.device_id),
            vendor => format!("{vendor} {:04x}", self.device_id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    pub connector: String,
    pub internal: bool,
    /// Preferred mode, e.g. `1920x1080`.
    pub mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    pub name: String,
    pub model: Option<String>,
    pub size_bytes: u64,
    pub removable: bool,
    pub rotational: Option<bool>,
    pub transport: DiskTransport,
    pub partitions: Vec<Partition>,
    pub usage: DiskUsage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub name: String,
    pub size_bytes: u64,
}

/// Why a disk may or may not be offered for installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskUsage {
    /// Nothing on it is in use by this session.
    Free,
    /// Holds the running system's root filesystem.
    HostRoot,
    /// Holds the media this session booted from (or another installer image).
    InstallationMedium,
    /// Mounted filesystems or active swap; never erased behind the user's back.
    InUse(Vec<String>),
}

impl fmt::Display for DiskUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiskUsage::Free => f.write_str("available"),
            DiskUsage::HostRoot => f.write_str("runs this system"),
            DiskUsage::InstallationMedium => f.write_str("installation medium"),
            DiskUsage::InUse(targets) => write!(f, "in use: {}", targets.join(", ")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskTransport {
    Nvme,
    Sata,
    Usb,
    Mmc,
    Virtio,
    Other,
}

impl fmt::Display for DiskTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DiskTransport::Nvme => "NVMe",
            DiskTransport::Sata => "SATA",
            DiskTransport::Usb => "USB",
            DiskTransport::Mmc => "MMC",
            DiskTransport::Virtio => "virtio",
            DiskTransport::Other => "other",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkAdapter {
    pub interface: String,
    pub wireless: bool,
    pub host_driver: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Touchscreen,
    Touchpad,
    Gamepad,
}

impl fmt::Display for InputKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            InputKind::Touchscreen => "touchscreen",
            InputKind::Touchpad => "touchpad",
            InputKind::Gamepad => "gamepad",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    pub name: String,
    pub kind: InputKind,
}

/// UEFI global variable vendor GUID.
const EFI_GLOBAL: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";

/// Block devices that are never installation targets.
const VIRTUAL_BLOCK: &[&str] = &["loop", "ram", "zram", "dm-", "md", "sr", "fd", "nbd"];

/// Filesystems that only installer and live media use.
const MEDIA_FS: &[&str] = &["iso9660", "udf", "squashfs"];

/// Scan the running machine.
pub fn scan() -> HardwareSnapshot {
    Scanner::new("/").scan()
}

/// One sysfs/procfs directory, read attribute by attribute.
struct Node(PathBuf);

impl Node {
    fn text(&self, attr: &str) -> Option<String> {
        fs::read_to_string(self.0.join(attr)).ok().map(|s| s.trim().to_owned())
    }

    fn hex(&self, attr: &str) -> Option<u32> {
        parse::hex(&self.text(attr)?)
    }

    fn sectors(&self) -> Option<u64> {
        self.text("size")?.parse().ok()
    }

    fn is(&self, attr: &str) -> bool {
        self.text(attr).as_deref() == Some("1")
    }

    fn has(&self, attr: &str) -> bool {
        self.0.join(attr).exists()
    }

    /// Whether efivarfs is mounted here and lists any variable at all.
    fn has_variables(&self) -> bool {
        fs::read_dir(self.0.join("efivars")).is_ok_and(|mut dir| dir.next().is_some())
    }

    /// Final component of a symlink target, e.g. a bound `driver`.
    fn link(&self, attr: &str) -> Option<String> {
        fs::read_link(self.0.join(attr)).ok().map(|t| name_of(&t))
    }

    fn name(&self) -> String {
        name_of(&self.0)
    }

    fn child(&self, name: &str) -> Node {
        Node(self.0.join(name))
    }

    /// Child nodes, sorted by name for stable output.
    fn children(&self) -> Vec<Node> {
        let mut paths: Vec<PathBuf> = fs::read_dir(&self.0)
            .map(|dir| dir.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        paths.sort();
        paths.into_iter().map(Node).collect()
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub struct Scanner {
    root: PathBuf,
}

impl Scanner {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Scanner { root: root.into() }
    }

    fn node(&self, rel: &str) -> Node {
        Node(self.root.join(rel))
    }

    fn proc(&self, file: &str) -> Option<String> {
        self.node("proc").text(file)
    }

    pub fn scan(&self) -> HardwareSnapshot {
        HardwareSnapshot {
            cpu: self.cpu(),
            memory_bytes: self.memory(),
            firmware: self.firmware(),
            secure_boot: self.secure_boot(),
            setup_mode: self.setup_mode(),
            device_class: self.device_class(),
            gpus: self.gpus(),
            displays: self.displays(),
            disks: self.disks(),
            network: self.network(),
            bluetooth: self.bluetooth(),
            inputs: self.inputs(),
            battery: self.battery(),
        }
    }

    pub fn cpu(&self) -> Fact<Cpu> {
        self.proc("cpuinfo").map_or_else(
            || Fact::unknown("/proc/cpuinfo is not readable"),
            |text| Fact::Known(parse::cpuinfo(&text)),
        )
    }

    pub fn memory(&self) -> Fact<u64> {
        self.proc("meminfo")
            .and_then(|text| parse::mem_total(&text))
            .map_or_else(|| Fact::unknown("MemTotal missing from /proc/meminfo"), Fact::Known)
    }

    pub fn firmware(&self) -> Fact<FirmwareMode> {
        let firmware = self.node("sys/firmware");
        match (firmware.0.is_dir(), firmware.has("efi")) {
            (false, _) => Fact::unknown("sysfs firmware directory is not available"),
            (true, false) => Fact::Known(FirmwareMode::Bios),
            (true, true) => Fact::Known(FirmwareMode::Uefi {
                bits: firmware.text("efi/fw_platform_size").and_then(|s| s.parse().ok()),
            }),
        }
    }

    pub fn secure_boot(&self) -> Fact<bool> {
        self.efi_flag("SecureBoot")
    }

    pub fn setup_mode(&self) -> Fact<bool> {
        self.efi_flag("SetupMode")
    }

    /// A one-byte boolean UEFI global variable.
    fn efi_flag(&self, name: &str) -> Fact<bool> {
        let efi = self.node("sys/firmware/efi");
        match fs::read(efi.0.join(format!("efivars/{name}-{EFI_GLOBAL}"))) {
            Ok(var) => parse::efi_bool(&var).map_or_else(
                || Fact::unknown(format!("{name} variable is truncated")),
                Fact::Known,
            ),
            // Without UEFI there is no Secure Boot, and no setup mode.
            Err(_) if !efi.0.is_dir() => Fact::Known(false),
            // Variables are readable but this one is absent: firmware without Secure
            // Boot support (UEFI 2.x §3.3 defines it only where supported).
            Err(_) if efi.has_variables() => Fact::Known(false),
            Err(_) => Fact::unknown(format!("{name} variable is not readable")),
        }
    }

    pub fn device_class(&self) -> Fact<DeviceClass> {
        let dmi = self.node("sys/class/dmi/id");
        // Virtual machines often name themselves only in the vendor field: QEMU's
        // product is "Standard PC (Q35 + ICH9, 2009)", its vendor "QEMU".
        let product = ["product_name", "sys_vendor"]
            .iter()
            .filter_map(|field| dmi.text(field))
            .collect::<Vec<_>>()
            .join(" ");
        let chassis = dmi.text("chassis_type").and_then(|c| c.parse().ok());
        parse::device_class(&product, chassis).map_or_else(Fact::unknown, Fact::Known)
    }

    pub fn gpus(&self) -> Vec<Gpu> {
        self.node("sys/bus/pci/devices")
            .children()
            .into_iter()
            // PCI base class 0x03: display controller.
            .filter(|dev| dev.hex("class").is_some_and(|class| class >> 16 == 0x03))
            .filter_map(|dev| {
                Some(Gpu {
                    vendor: GpuVendor::from_pci(dev.hex("vendor")? as u16),
                    device_id: dev.hex("device")? as u16,
                    host_driver: dev.link("driver"),
                    boot_display: dev.is("boot_vga"),
                    pci_slot: dev.name(),
                })
            })
            .collect()
    }

    /// Connected outputs, built-in panels first.
    pub fn displays(&self) -> Vec<Display> {
        let mut displays: Vec<Display> = self
            .node("sys/class/drm")
            .children()
            .into_iter()
            .filter(|out| out.text("status").as_deref() == Some("connected"))
            .filter_map(|out| {
                let connector = parse::connector(&out.name())?.to_owned();
                Some(Display {
                    internal: parse::internal_connector(&connector),
                    mode: out.text("modes").and_then(|m| m.lines().next().map(str::to_owned)),
                    connector,
                })
            })
            .collect();
        displays.sort_by_key(|d| !d.internal);
        displays
    }

    pub fn disks(&self) -> Vec<Disk> {
        let mounts: Vec<Mount> = [
            self.proc("mounts").map(|t| parse::mounts(&t)),
            self.proc("swaps").map(|t| parse::swaps(&t)),
        ]
        .into_iter()
        .flatten()
        .flatten()
        .collect();

        self.node("sys/block")
            .children()
            .into_iter()
            .filter(|dev| !VIRTUAL_BLOCK.iter().any(|p| dev.name().starts_with(p)))
            .filter_map(|dev| {
                let sectors = dev.sectors().filter(|s| *s > 0)?;
                let name = dev.name();
                let partitions: Vec<Partition> = dev
                    .children()
                    .into_iter()
                    .filter(|part| part.has("partition"))
                    .map(|part| Partition {
                        size_bytes: part.sectors().unwrap_or(0) * 512,
                        name: part.name(),
                    })
                    .collect();
                Some(Disk {
                    transport: self.transport(&dev, &name),
                    model: dev.text("device/model").filter(|m| !m.is_empty()),
                    // sysfs counts 512-byte units regardless of logical block size.
                    size_bytes: sectors * 512,
                    removable: dev.is("removable"),
                    rotational: dev.text("queue/rotational").map(|r| r == "1"),
                    usage: self.usage(&dev, &partitions, &mounts),
                    partitions,
                    name,
                })
            })
            .collect()
    }

    fn transport(&self, dev: &Node, name: &str) -> DiskTransport {
        let path = fs::canonicalize(dev.0.join("device")).unwrap_or_default();
        let on_usb = path.components().any(|c| c.as_os_str().to_string_lossy().starts_with("usb"));
        match name {
            _ if on_usb => DiskTransport::Usb,
            n if n.starts_with("nvme") => DiskTransport::Nvme,
            n if n.starts_with("sd") => DiskTransport::Sata,
            n if n.starts_with("mmcblk") => DiskTransport::Mmc,
            n if n.starts_with("vd") => DiskTransport::Virtio,
            _ => DiskTransport::Other,
        }
    }

    /// Every device-mapper or md device stacked on `dev`, by `/dev` name and, for
    /// device-mapper, by `/dev/mapper` name.
    fn stacked(&self, dev: &Node, into: &mut BTreeSet<String>) {
        for holder in dev.child("holders").children() {
            let name = holder.name();
            let block = self.node("sys/block").child(&name);
            if let Some(mapped) = block.text("dm/name") {
                into.insert(format!("mapper/{mapped}"));
            }
            if into.insert(name) {
                self.stacked(&block, into);
            }
        }
    }

    fn usage(&self, dev: &Node, partitions: &[Partition], mounts: &[Mount]) -> DiskUsage {
        let mut names: BTreeSet<String> = BTreeSet::from([dev.name()]);
        self.stacked(dev, &mut names);
        for part in partitions {
            names.insert(part.name.clone());
            self.stacked(&dev.child(&part.name), &mut names);
        }
        let on_disk: Vec<&Mount> = mounts
            .iter()
            .filter(|m| m.source.strip_prefix("/dev/").is_some_and(|n| names.contains(n)))
            .collect();

        if on_disk.iter().any(|m| m.target == "/") {
            DiskUsage::HostRoot
        } else if on_disk.iter().any(|m| MEDIA_FS.contains(&m.fstype.as_str())) {
            DiskUsage::InstallationMedium
        } else if on_disk.is_empty() {
            DiskUsage::Free
        } else {
            DiskUsage::InUse(on_disk.iter().map(|m| m.target.clone()).collect())
        }
    }

    pub fn network(&self) -> Vec<NetworkAdapter> {
        self.node("sys/class/net")
            .children()
            .into_iter()
            // Only physical adapters have a backing device.
            .filter(|dev| dev.has("device"))
            .map(|dev| NetworkAdapter {
                wireless: dev.has("wireless") || dev.has("phy80211"),
                host_driver: dev.link("device/driver"),
                interface: dev.name(),
            })
            .collect()
    }

    /// Bluetooth controllers (`hci0`), not their connections (`hci0:256`).
    pub fn bluetooth(&self) -> Vec<String> {
        self.node("sys/class/bluetooth")
            .children()
            .into_iter()
            .map(|hci| hci.name())
            .filter(|name| !name.contains(':'))
            .collect()
    }

    pub fn inputs(&self) -> Vec<InputDevice> {
        let text = self.node("proc/bus/input").text("devices").unwrap_or_default();
        parse::input_devices(&text)
            .into_iter()
            .map(|(name, kind)| InputDevice { name, kind })
            .collect()
    }

    /// A system battery, as opposed to a peripheral's (`scope = Device`).
    pub fn battery(&self) -> bool {
        self.node("sys/class/power_supply").children().iter().any(|supply| {
            supply.text("type").as_deref() == Some("Battery")
                && supply.text("scope").as_deref() != Some("Device")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use borea_testkit::FakeRoot;

    const ZEN4_FLAGS: &str = "fpu sse2 ssse3 fma cx16 sse4_1 sse4_2 movbe popcnt xsave avx f16c \
        lahf_lm abm bmi1 avx2 bmi2 avx512f avx512dq avx512cd avx512bw avx512vl";

    #[test]
    fn empty_root_reports_unknown_not_unsupported() {
        let root = FakeRoot::new();
        let hw = Scanner::new(root.path()).scan();
        assert!(matches!(hw.cpu, Fact::Unknown { .. }));
        assert!(matches!(hw.firmware, Fact::Unknown { .. }));
        assert!(matches!(hw.device_class, Fact::Unknown { .. }));
        assert!(hw.gpus.is_empty() && hw.disks.is_empty() && hw.network.is_empty());
        assert!(!hw.battery && hw.inputs.is_empty() && hw.displays.is_empty());
    }

    #[test]
    fn firmware_without_secure_boot_support_is_off_not_unknown() {
        let root = FakeRoot::new();
        root.file("sys/firmware/efi/fw_platform_size", "64\n")
            .file(&format!("sys/firmware/efi/efivars/SetupMode-{EFI_GLOBAL}"), [6, 0, 0, 0, 0])
            .file("sys/class/dmi/id/product_name", "Standard PC (Q35 + ICH9, 2009)\n")
            .file("sys/class/dmi/id/sys_vendor", "QEMU\n")
            .file("sys/class/dmi/id/chassis_type", "1\n");
        let scanner = Scanner::new(root.path());
        assert_eq!(scanner.secure_boot(), Fact::Known(false));
        assert_eq!(scanner.device_class(), Fact::Known(DeviceClass::VirtualMachine));

        // efivarfs not mounted: nothing can be said.
        let root = FakeRoot::new();
        root.file("sys/firmware/efi/fw_platform_size", "64\n");
        assert!(Scanner::new(root.path()).secure_boot().known().is_none());
    }

    #[test]
    fn handheld_uefi_fixture() {
        let secure_boot_var = format!("sys/firmware/efi/efivars/SecureBoot-{EFI_GLOBAL}");
        let gpu = "sys/bus/pci/devices/0000:04:00.0";
        let root = FakeRoot::new();
        root.file(
            "proc/cpuinfo",
            format!(
                "processor\t: 0\nvendor_id\t: AuthenticAMD\nmodel name\t: AMD Custom APU 0405\nflags\t\t: {ZEN4_FLAGS}\n\nprocessor\t: 1\n"
            ),
        )
        .file("proc/meminfo", "MemTotal:       16000000 kB\n")
        .file("sys/firmware/efi/fw_platform_size", "64\n")
        .file(&secure_boot_var, [0x06, 0, 0, 0, 1])
        .file(&format!("sys/firmware/efi/efivars/SetupMode-{EFI_GLOBAL}"), [0x06, 0, 0, 0, 0])
        .file("sys/class/dmi/id/product_name", "Jupiter\n")
        .file("sys/class/dmi/id/chassis_type", "10\n")
        .file(&format!("{gpu}/class"), "0x030000\n")
        .file(&format!("{gpu}/vendor"), "0x1002\n")
        .file(&format!("{gpu}/device"), "0x1435\n")
        .file(&format!("{gpu}/boot_vga"), "1\n")
        .dir("sys/bus/pci/drivers/amdgpu")
        .link(&format!("{gpu}/driver"), "sys/bus/pci/drivers/amdgpu")
        .file("sys/bus/pci/devices/0000:00:01.0/class", "0x060400\n")
        .file("sys/bus/pci/devices/0000:00:01.0/vendor", "0x1022\n")
        .file("sys/class/drm/card0-eDP-1/status", "connected\n")
        .file("sys/class/drm/card0-eDP-1/modes", "800x1280\n640x480\n")
        .file("sys/class/drm/card0-DP-1/status", "disconnected\n")
        .file("sys/class/power_supply/BAT1/type", "Battery\n")
        .file("sys/class/power_supply/BAT1/scope", "System\n")
        .dir("sys/class/bluetooth/hci0")
        .dir("sys/class/bluetooth/hci0:256")
        .file(
            "proc/bus/input/devices",
            "N: Name=\"Steam Deck\"\nB: PROP=0\nB: KEY=7fdb000000000000 0 0 0 0\n",
        )
        .file("sys/class/net/wlan0/device/.keep", "")
        .dir("sys/class/net/wlan0/wireless")
        .dir("sys/class/net/lo");

        let hw = Scanner::new(root.path()).scan();
        let cpu = hw.cpu.known().unwrap();
        assert_eq!(cpu.x86_level, Some(X86Level::V4));
        assert_eq!(cpu.logical_cores, 2);
        assert_eq!(cpu.microcode(), Some(Microcode::Amd));
        assert_eq!(hw.memory_bytes, Fact::Known(16_000_000 * 1024));
        assert_eq!(hw.firmware, Fact::Known(FirmwareMode::Uefi { bits: Some(64) }));
        assert_eq!(hw.secure_boot, Fact::Known(true));
        assert_eq!(hw.setup_mode, Fact::Known(false));
        assert_eq!(hw.device_class, Fact::Known(DeviceClass::Handheld));
        assert_eq!(hw.gpus.len(), 1, "bridges are not GPUs");
        assert_eq!(hw.gpus[0].host_driver.as_deref(), Some("amdgpu"));
        assert!(!hw.hybrid_graphics());
        assert_eq!(
            hw.displays,
            [Display { connector: "eDP-1".into(), internal: true, mode: Some("800x1280".into()) }]
        );
        assert!(hw.battery);
        assert_eq!(hw.bluetooth, ["hci0"]);
        assert_eq!(hw.inputs[0].kind, InputKind::Gamepad);
        assert_eq!(hw.network.len(), 1, "virtual interfaces are excluded");
        assert!(hw.network[0].wireless);

        root.file(&secure_boot_var, [0x06, 0, 0, 0, 0]);
        assert_eq!(Scanner::new(root.path()).secure_boot(), Fact::Known(false));
    }

    /// Recorded layout: encrypted root on NVMe, a booted installer stick, a spare SSD.
    #[test]
    fn disk_usage_protects_the_running_system_and_boot_media() {
        let root = FakeRoot::new();
        root.file("sys/block/nvme0n1/size", "1953525168\n")
            .file("sys/block/nvme0n1/nvme0n1p1/partition", "1\n")
            .file("sys/block/nvme0n1/nvme0n1p1/size", "4194304\n")
            .file("sys/block/nvme0n1/nvme0n1p2/partition", "2\n")
            .file("sys/block/nvme0n1/nvme0n1p2/size", "1949328000\n")
            .dir("sys/block/nvme0n1/nvme0n1p2/holders/dm-0")
            .file("sys/block/dm-0/size", "1949295232\n")
            .file("sys/block/dm-0/dm/name", "root\n")
            .file("sys/block/sda/size", "123207680\n")
            .file("sys/block/sda/removable", "1\n")
            .file("sys/block/sda/sda1/partition", "1\n")
            .file("sys/block/sda/sda1/size", "123207616\n")
            .file("sys/block/sdb/size", "500118192\n")
            .file("sys/block/sdc/size", "500118192\n")
            .file("sys/block/sdc/sdc1/partition", "1\n")
            .file("sys/block/sdc/sdc1/size", "500000000\n")
            .file(
                "proc/mounts",
                "/dev/mapper/root / btrfs rw 0 0\n\
                 /dev/nvme0n1p1 /boot vfat rw 0 0\n\
                 /dev/sda1 /run/archiso/bootmnt iso9660 ro 0 0\n\
                 /dev/sdc1 /mnt/data ext4 rw 0 0\n",
            );

        let disks = Scanner::new(root.path()).disks();
        let usage = |name: &str| disks.iter().find(|d| d.name == name).unwrap().usage.clone();
        assert_eq!(usage("nvme0n1"), DiskUsage::HostRoot, "root through LUKS/dm is found");
        assert_eq!(usage("sda"), DiskUsage::InstallationMedium);
        assert_eq!(usage("sdb"), DiskUsage::Free);
        assert_eq!(usage("sdc"), DiskUsage::InUse(vec!["/mnt/data".into()]));
        assert!(!disks.iter().any(|d| d.name.starts_with("dm-")));
        assert_eq!(disks.iter().find(|d| d.name == "nvme0n1").unwrap().partitions.len(), 2);
    }
}
