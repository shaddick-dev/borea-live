//! Pure parsers from kernel-provided text to facts. No I/O, trivially testable.

use borea_core::{Architecture, DeviceClass, X86Level};

use crate::{Cpu, InputKind};

/// psABI level requirements, each level adding to the one before.
/// cpuinfo spells LZCNT as `abm` and LAHF/SAHF as `lahf_lm`.
const LEVELS: [(X86Level, &[&str]); 3] = [
    (X86Level::V2, &["cx16", "lahf_lm", "popcnt", "sse4_1", "sse4_2", "ssse3"]),
    (X86Level::V3, &["avx", "avx2", "bmi1", "bmi2", "f16c", "fma", "abm", "movbe", "xsave"]),
    (X86Level::V4, &["avx512f", "avx512bw", "avx512cd", "avx512dq", "avx512vl"]),
];

/// Highest x86-64 level supported by a cpuinfo `flags` line.
pub fn x86_level(flags: &str) -> X86Level {
    let has = |flag: &&str| flags.split_whitespace().any(|f| f == *flag);
    LEVELS
        .iter()
        .take_while(|(_, needed)| needed.iter().all(has))
        .last()
        .map_or(X86Level::V1, |(level, _)| *level)
}

/// `/proc/cpuinfo`. Only the first processor block is read for identity.
pub fn cpuinfo(text: &str) -> Cpu {
    let first = text.split("\n\n").next().unwrap_or_default();
    let field = |name: &str| {
        first.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == name).then(|| value.trim().to_owned())
        })
    };
    let flags = field("flags");
    // x86 kernels publish `flags`; arm64 kernels publish `Features`.
    let architecture = match (&flags, field("Features")) {
        (Some(_), _) => Architecture::X86_64,
        (None, Some(_)) => Architecture::Aarch64,
        (None, None) => Architecture::from_uname(std::env::consts::ARCH),
    };
    Cpu {
        architecture,
        model: field("model name"),
        vendor: field("vendor_id"),
        x86_level: flags.as_deref().map(x86_level),
        logical_cores: text.lines().filter(|l| l.starts_with("processor")).count(),
    }
}

/// `MemTotal` from `/proc/meminfo`, in bytes.
pub fn mem_total(text: &str) -> Option<u64> {
    let kib = text.lines().find_map(|l| l.strip_prefix("MemTotal:"))?;
    kib.split_whitespace().next()?.parse::<u64>().ok().map(|kib| kib * 1024)
}

/// efivarfs value: four attribute bytes, then the variable's data.
pub fn efi_bool(var: &[u8]) -> Option<bool> {
    var.get(4).map(|&value| value == 1)
}

/// Device class from DMI product name and vendor, and SMBIOS chassis type
/// (DSP0134 §7.4.1).
pub fn device_class(product: &str, chassis: Option<u8>) -> Result<DeviceClass, String> {
    // DMI product names of known handhelds: Steam Deck LCD/OLED, ROG Ally/Ally X,
    // Legion Go. Extend from recorded hardware fixtures.
    const HANDHELDS: &[&str] = &["Jupiter", "Galileo", "RC71L", "RC72L", "83E1"];
    const VIRTUAL: &[&str] = &["QEMU", "KVM", "VirtualBox", "VMware", "Virtual Machine"];

    let named = |names: &[&str]| names.iter().any(|n| product.contains(n));
    match chassis {
        _ if named(HANDHELDS) => Ok(DeviceClass::Handheld),
        Some(11) => Ok(DeviceClass::Handheld),
        Some(3..=7 | 13 | 15 | 16 | 24 | 34 | 35 | 36) => Ok(DeviceClass::Desktop),
        Some(8..=10 | 14 | 31 | 32) => Ok(DeviceClass::Laptop),
        Some(30) => Ok(DeviceClass::Tablet),
        Some(17 | 23 | 25 | 28 | 29) => Ok(DeviceClass::Server),
        _ if named(VIRTUAL) => Ok(DeviceClass::VirtualMachine),
        Some(1 | 2) | None => Err("the firmware does not say what kind of device this is".into()),
        Some(other) => Err(format!("unusual chassis type {other}")),
    }
}

/// A sysfs hex attribute such as `0x10de`.
pub fn hex(text: &str) -> Option<u32> {
    u32::from_str_radix(text.trim_start_matches("0x"), 16).ok()
}

/// One `/proc/mounts` or `/proc/swaps` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    pub source: String,
    pub target: String,
    pub fstype: String,
}

/// `/proc/mounts`, with octal escapes (`\040` for space) decoded.
pub fn mounts(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some(Mount {
                source: unescape(fields.next()?),
                target: unescape(fields.next()?),
                fstype: fields.next()?.to_owned(),
            })
        })
        .collect()
}

/// `/proc/swaps`, as mounts whose target is `[swap]`.
pub fn swaps(text: &str) -> Vec<Mount> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let source = unescape(line.split_whitespace().next()?);
            Some(Mount { source, target: "[swap]".into(), fstype: "swap".into() })
        })
        .collect()
}

fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut rest = field;
    while let Some(i) = rest.find('\\') {
        out.push_str(&rest[..i]);
        match rest.get(i + 1..i + 4).and_then(|oct| u8::from_str_radix(oct, 8).ok()) {
            Some(byte) => {
                out.push(byte as char);
                rest = &rest[i + 4..];
            }
            None => {
                out.push('\\');
                rest = &rest[i + 1..];
            }
        }
    }
    out + rest
}

/// Input event codes and properties from `linux/input-event-codes.h`.
const BTN_GAMEPAD: usize = 0x130;
const BTN_TOUCH: usize = 0x14a;
const INPUT_PROP_POINTER: u32 = 1 << 0;
const INPUT_PROP_DIRECT: u32 = 1 << 1;
const BUS_VIRTUAL: u32 = 0x06;

/// Touch and game input devices from `/proc/bus/input/devices`.
///
/// Classified by capability bits rather than names or handlers: a gaming keyboard
/// that registers a joystick handler is not a gamepad unless it reports gamepad
/// buttons.
pub fn input_devices(text: &str) -> Vec<(String, InputKind)> {
    text.split("\n\n")
        .filter_map(|block| {
            let field =
                |prefix: &str| block.lines().find_map(|l| l.strip_prefix(prefix)).map(str::trim);
            // Software-made devices (uinput tools, remappers) are not hardware.
            let bus = field("I: Bus=").and_then(|i| i.split_whitespace().next()).and_then(hex);
            if bus == Some(BUS_VIRTUAL)
                || field("S: Sysfs=").is_some_and(|s| s.contains("/virtual/"))
            {
                return None;
            }
            let name = field("N: Name=")?.trim_matches('"').trim().to_owned();
            let props = field("B: PROP=").and_then(hex).unwrap_or(0);
            let keys = field("B: KEY=").unwrap_or("");
            let kind = if has_bit(keys, BTN_GAMEPAD) {
                InputKind::Gamepad
            } else if has_bit(keys, BTN_TOUCH) && props & INPUT_PROP_DIRECT != 0 {
                InputKind::Touchscreen
            } else if has_bit(keys, BTN_TOUCH) && props & INPUT_PROP_POINTER != 0 {
                InputKind::Touchpad
            } else {
                return None;
            };
            Some((name, kind))
        })
        .collect()
}

/// Whether a capability bitmap (hex words, most significant first, each the width
/// of a kernel `long`) has `bit` set.
fn has_bit(bitmap: &str, bit: usize) -> bool {
    const WORD: usize = u64::BITS as usize;
    bitmap
        .split_whitespace()
        .rev()
        .nth(bit / WORD)
        .and_then(|word| u64::from_str_radix(word, 16).ok())
        .is_some_and(|word| word & (1 << (bit % WORD)) != 0)
}

/// Connector name from a DRM sysfs entry: `card1-eDP-1` → `eDP-1`.
pub fn connector(entry: &str) -> Option<&str> {
    let (card, name) = entry.split_once('-')?;
    card.starts_with("card").then_some(name)
}

/// Built-in panels connect over eDP, LVDS, or DSI.
pub fn internal_connector(name: &str) -> bool {
    ["eDP", "LVDS", "DSI"].iter().any(|p| name.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZEN4_FLAGS: &str = "fpu sse2 ssse3 fma cx16 sse4_1 sse4_2 movbe popcnt xsave avx f16c \
        lahf_lm abm bmi1 avx2 bmi2 avx512f avx512dq avx512cd avx512bw avx512vl";

    #[test]
    fn levels_from_flags() {
        assert_eq!(x86_level("fpu sse sse2"), X86Level::V1);
        assert_eq!(x86_level("cx16 lahf_lm popcnt sse4_1 sse4_2 ssse3"), X86Level::V2);
        assert_eq!(x86_level(&ZEN4_FLAGS.replace(" avx512vl", "")), X86Level::V3);
        assert_eq!(x86_level(ZEN4_FLAGS), X86Level::V4);
        // AVX-512 without the v2 baseline does not skip levels.
        assert_eq!(x86_level("avx512f avx512bw avx512cd avx512dq avx512vl"), X86Level::V1);
    }

    #[test]
    fn device_classes() {
        assert_eq!(device_class("Jupiter", Some(10)), Ok(DeviceClass::Handheld));
        assert_eq!(device_class("ThinkPad", Some(10)), Ok(DeviceClass::Laptop));
        assert_eq!(
            device_class("Standard PC (Q35 + ICH9, 2009) QEMU", Some(1)),
            Ok(DeviceClass::VirtualMachine)
        );
        assert!(device_class("Mystery", Some(2)).is_err());
        assert!(device_class("", None).is_err());
    }

    #[test]
    fn mounts_decode_escapes() {
        let parsed = mounts("/dev/sda1 /run/media/me/My\\040Stick vfat rw 0 0\n");
        assert_eq!(parsed[0].target, "/run/media/me/My Stick");
        let swap = swaps("Filename Type Size Used Priority\n/dev/nvme0n1p3 partition 1 0 -2\n");
        assert_eq!(swap[0].source, "/dev/nvme0n1p3");
    }

    // Recorded from a laptop: a gaming keyboard exposes `js0` but no gamepad buttons.
    const INPUT: &str = r#"I: Bus=0003 Vendor=1b1c Product=1bb9 Version=0100
N: Name="Corsair K70 Gaming Keyboard Mouse"
H: Handlers=event5 js0 mouse0
B: PROP=0
B: KEY=ffffffff0001 0 0 0 0

I: Bus=0018 Vendor=2808 Product=0202 Version=0100
N: Name="ASUF1204:00 2808:0202 Touchpad"
H: Handlers=event22 mouse4
B: PROP=5
B: KEY=e520 10000 0 0 0 0

I: Bus=0003 Vendor=28de Product=1205 Version=0111
N: Name="Steam Deck"
H: Handlers=event9 js1
B: PROP=0
B: KEY=7fdb000000000000 0 0 0 0

I: Bus=0018 Vendor=2808 Product=1015 Version=0100
N: Name="FTS3528:00 2808:1015"
H: Handlers=event10
B: PROP=2
B: KEY=400 0 0 0 0 0

I: Bus=0006 Vendor=2333 Product=6666 Version=0001
N: Name="ydotoold virtual device"
S: Sysfs=/devices/virtual/input/input39
B: PROP=0
B: KEY=ffffffffff 0 ffffff0003007f 1000f7fffffff 7fe001fffff000f 7ffffffffffffff
"#;

    #[test]
    fn input_classified_by_capability() {
        assert_eq!(
            input_devices(INPUT),
            [
                ("ASUF1204:00 2808:0202 Touchpad".to_owned(), InputKind::Touchpad),
                ("Steam Deck".to_owned(), InputKind::Gamepad),
                ("FTS3528:00 2808:1015".to_owned(), InputKind::Touchscreen),
            ]
        );
    }

    #[test]
    fn drm_connectors() {
        assert_eq!(connector("card1-eDP-1"), Some("eDP-1"));
        assert_eq!(connector("renderD128"), None);
        assert!(internal_connector("eDP-1") && !internal_connector("HDMI-A-1"));
    }
}
