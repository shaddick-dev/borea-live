//! The installation environment: where Borea is running, independent of what it installs.
//!
//! Borea is not tied to any distribution's ISO. It runs from Borea Live (the reference
//! environment) or from any compatible live session, and judges that host only by
//! *capabilities*: kernel features, firmware access, services. The host's distribution
//! identity is recorded for diagnostics and never decides which targets can be
//! installed. An Arch live session can install Fedora; a Fedora live session can
//! install Arch.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::PathBuf;

/// What kind of environment Borea is running in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKind {
    /// Borea's own live environment: the predictable, reference host.
    BoreaLive { version: String },
    /// Another distribution's live session. Works when its capabilities suffice;
    /// drivers, firmware, and services are whatever that session ships.
    LiveSession,
    /// A normally installed system. Installing to a second disk from here is a later
    /// capability; installing over the running root is never offered.
    InstalledSystem,
}

impl HostKind {
    pub fn is_live(&self) -> bool {
        !matches!(self, HostKind::InstalledSystem)
    }
}

impl fmt::Display for HostKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostKind::BoreaLive { version } => write!(f, "Borea Live {version}"),
            HostKind::LiveSession => f.write_str("compatible live session"),
            HostKind::InstalledSystem => f.write_str("installed system"),
        }
    }
}

/// A capability of the host that installation work can depend on.
///
/// Deliberately absent: "has pacman", "has dnf", "is Arch". Native tools come from
/// the target's own upstream sources, fetched by its adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HostCapability {
    /// UEFI runtime variables are exposed, so boot entries can be created.
    EfiVariables,
    /// Private mount namespaces for an isolated target mount tree.
    MountNamespaces,
    /// Loop devices, for reading upstream images used as sources.
    LoopDevices,
    KernelVfat,
    KernelExt4,
    KernelBtrfs,
    KernelSquashfs,
    KernelOverlay,
    KernelErofs,
    /// The D-Bus system bus is running.
    SystemBus,
    /// NetworkManager is running, so Windpad and the TUI can manage Wi-Fi.
    NetworkManager,
    /// polkit is installed, for graphical elevation of the privileged worker.
    Polkit,
    /// A Wayland or X11 session is available for the Windpad GUI.
    GraphicalSession,
}

impl fmt::Display for HostCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use HostCapability::*;
        f.write_str(match self {
            EfiVariables => "UEFI variables",
            MountNamespaces => "mount namespaces",
            LoopDevices => "loop devices",
            KernelVfat => "vfat",
            KernelExt4 => "ext4",
            KernelBtrfs => "btrfs",
            KernelSquashfs => "squashfs",
            KernelOverlay => "overlayfs",
            KernelErofs => "erofs",
            SystemBus => "D-Bus system bus",
            NetworkManager => "NetworkManager",
            Polkit => "polkit",
            GraphicalSession => "graphical session",
        })
    }
}

/// How a capability is recognised. Every probe is a read-only lookup.
enum Probe {
    /// Any of these paths exists.
    Path(&'static [&'static str]),
    /// The kernel has this filesystem: registered now, or shipped as a module (named
    /// by its path under `kernel/fs/`) that loads on first mount.
    Filesystem { name: &'static str, module: &'static str },
    /// Any of these environment variables is set and non-empty.
    Env(&'static [&'static str]),
    /// Every listed probe passes.
    All(&'static [Probe]),
}

const SYSTEM_BUS: Probe = Probe::Path(&["run/dbus/system_bus_socket"]);

const fn fs(name: &'static str, module: &'static str) -> Probe {
    Probe::Filesystem { name, module }
}

const PROBES: &[(HostCapability, Probe)] = {
    use HostCapability::*;
    &[
        (EfiVariables, Probe::Path(&["sys/firmware/efi/efivars"])),
        (MountNamespaces, Probe::Path(&["proc/self/ns/mnt"])),
        (LoopDevices, Probe::Path(&["dev/loop-control"])),
        (KernelVfat, fs("vfat", "fat/vfat")),
        (KernelExt4, fs("ext4", "ext4/ext4")),
        (KernelBtrfs, fs("btrfs", "btrfs/btrfs")),
        (KernelSquashfs, fs("squashfs", "squashfs/squashfs")),
        (KernelOverlay, fs("overlay", "overlayfs/overlay")),
        (KernelErofs, fs("erofs", "erofs/erofs")),
        (SystemBus, SYSTEM_BUS),
        (NetworkManager, Probe::All(&[SYSTEM_BUS, Probe::Path(&["run/NetworkManager"])])),
        (
            Polkit,
            Probe::Path(&[
                "usr/lib/polkit-1/polkitd",
                "usr/libexec/polkitd",
                "usr/libexec/polkit-1/polkitd",
            ]),
        ),
        (GraphicalSession, Probe::Env(&["WAYLAND_DISPLAY", "DISPLAY"])),
    ]
};

/// Kernel command-line markers of common live-media frameworks
/// (archiso, dracut dmsquash-live, live-boot, casper).
const LIVE_CMDLINE: &[&str] =
    &["archisobasedir=", "rd.live.image", "root=live:", "boot=live", "boot=casper"];

/// Root filesystem types that only a live session mounts at `/`.
const LIVE_ROOT_FS: &[&str] = &["overlay", "squashfs", "tmpfs", "rootfs"];

/// Identity of the host OS from os-release. Informational only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsIdentity {
    pub id: String,
    pub pretty_name: String,
}

#[derive(Debug, Clone)]
pub struct HostEnvironment {
    pub kind: HostKind,
    pub os: Option<OsIdentity>,
    pub capabilities: BTreeSet<HostCapability>,
}

impl HostEnvironment {
    pub fn has(&self, capability: HostCapability) -> bool {
        self.capabilities.contains(&capability)
    }

    /// Capabilities from `needed` that this host lacks.
    pub fn missing<'a>(
        &self,
        needed: impl IntoIterator<Item = &'a HostCapability>,
    ) -> Vec<HostCapability> {
        needed.into_iter().copied().filter(|c| !self.has(*c)).collect()
    }
}

/// Probe the environment Borea is running in.
pub fn detect() -> HostEnvironment {
    HostProbe { root: PathBuf::from("/"), env: std::env::vars().collect() }.detect()
}

/// Host detection beneath a configurable root and environment, for fixtures.
pub struct HostProbe {
    pub root: PathBuf,
    pub env: Vec<(String, String)>,
}

/// Kernel facts read once and shared by every filesystem probe.
struct Kernel {
    registered: String,
    modules: String,
}

impl HostProbe {
    fn read(&self, rel: &str) -> Option<String> {
        fs::read_to_string(self.root.join(rel)).ok()
    }

    fn exists(&self, rel: &str) -> bool {
        self.root.join(rel).exists()
    }

    fn env_set(&self, key: &str) -> bool {
        self.env.iter().any(|(k, v)| k == key && !v.is_empty())
    }

    pub fn detect(&self) -> HostEnvironment {
        let kernel = self.kernel();
        HostEnvironment {
            kind: self.kind(),
            os: self.os(),
            capabilities: PROBES
                .iter()
                .filter(|(_, probe)| self.passes(probe, &kernel))
                .map(|(capability, _)| *capability)
                .collect(),
        }
    }

    fn passes(&self, probe: &Probe, kernel: &Kernel) -> bool {
        match probe {
            Probe::Path(paths) => paths.iter().any(|p| self.exists(p)),
            Probe::Env(keys) => keys.iter().any(|k| self.env_set(k)),
            Probe::All(probes) => probes.iter().all(|p| self.passes(p, kernel)),
            Probe::Filesystem { name, module } => {
                kernel.registered.lines().any(|l| l.split_whitespace().last() == Some(name))
                    || kernel.modules.contains(&format!("kernel/fs/{module}.ko"))
            }
        }
    }

    fn kernel(&self) -> Kernel {
        let modules = self.read("proc/sys/kernel/osrelease").map(|release| {
            ["modules.dep", "modules.builtin"]
                .iter()
                .filter_map(|index| self.read(&format!("lib/modules/{}/{index}", release.trim())))
                .collect()
        });
        Kernel {
            registered: self.read("proc/filesystems").unwrap_or_default(),
            modules: modules.unwrap_or_default(),
        }
    }

    fn kind(&self) -> HostKind {
        if let Some(marker) = self.read("etc/borea-live") {
            let version = marker.trim();
            let version = if version.is_empty() { "dev" } else { version };
            return HostKind::BoreaLive { version: version.into() };
        }
        let cmdline = self.read("proc/cmdline").unwrap_or_default();
        let live_cmdline =
            cmdline.split_whitespace().any(|arg| LIVE_CMDLINE.iter().any(|m| arg.starts_with(m)));
        let live_root = self.read("proc/mounts").is_some_and(|mounts| {
            mounts.lines().any(|line| {
                let fields: Vec<_> = line.split_whitespace().take(3).collect();
                matches!(fields[..], [_, "/", fstype] if LIVE_ROOT_FS.contains(&fstype))
            })
        });
        if live_cmdline || live_root { HostKind::LiveSession } else { HostKind::InstalledSystem }
    }

    fn os(&self) -> Option<OsIdentity> {
        let text = self.read("etc/os-release").or_else(|| self.read("usr/lib/os-release"))?;
        let value = |key: &str| {
            text.lines().find_map(|line| {
                let (k, v) = line.split_once('=')?;
                (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
            })
        };
        let id = value("ID")?;
        Some(OsIdentity { pretty_name: value("PRETTY_NAME").unwrap_or_else(|| id.clone()), id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use HostCapability::*;
    use borea_testkit::FakeRoot;

    fn probe(root: &FakeRoot, env: &[(&str, &str)]) -> HostEnvironment {
        let env = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        HostProbe { root: root.path().to_owned(), env }.detect()
    }

    #[test]
    fn borea_live_marker_wins() {
        let root = FakeRoot::new();
        root.file("etc/borea-live", "0.1.0\n")
            .file("proc/cmdline", "root=live:CDLABEL=BOREA rd.live.image");
        assert_eq!(probe(&root, &[]).kind, HostKind::BoreaLive { version: "0.1.0".into() });
    }

    #[test]
    fn any_distributions_live_media_is_a_live_session() {
        for cmdline in [
            "BOOT_IMAGE=/arch/boot/x86_64/vmlinuz-linux archisobasedir=arch archisolabel=ARCH",
            "BOOT_IMAGE=/images/pxeboot/vmlinuz root=live:CDLABEL=Fedora rd.live.image quiet",
            "BOOT_IMAGE=/live/vmlinuz boot=live components quiet",
        ] {
            let root = FakeRoot::new();
            root.file("proc/cmdline", cmdline);
            assert_eq!(probe(&root, &[]).kind, HostKind::LiveSession, "{cmdline}");
        }
    }

    #[test]
    fn overlay_root_is_live_and_disk_root_is_installed() {
        let root = FakeRoot::new();
        root.file("proc/mounts", "overlay / overlay rw 0 0\nproc /proc proc rw 0 0\n");
        assert_eq!(probe(&root, &[]).kind, HostKind::LiveSession);
        root.file("proc/mounts", "/dev/nvme0n1p2 / btrfs rw 0 0\n");
        assert_eq!(probe(&root, &[]).kind, HostKind::InstalledSystem);
    }

    #[test]
    fn capabilities_come_from_kernel_and_services_not_distro() {
        let root = FakeRoot::new();
        root.file(
            "etc/os-release",
            "NAME=\"Fedora Linux\"\nID=fedora\nPRETTY_NAME=\"Fedora Linux 44\"\n",
        )
        .file(
            "proc/filesystems",
            "nodev\tsysfs\n\tvfat\n\text4\n\tbtrfs\nnodev\toverlay\n\tsquashfs\n",
        )
        .dir("sys/firmware/efi/efivars")
        .file("run/dbus/system_bus_socket", "")
        .dir("run/NetworkManager");
        let host = probe(&root, &[("WAYLAND_DISPLAY", "wayland-0")]);

        assert_eq!(host.os.as_ref().unwrap().id, "fedora");
        for cap in [
            EfiVariables,
            KernelVfat,
            KernelExt4,
            KernelBtrfs,
            KernelOverlay,
            KernelSquashfs,
            SystemBus,
            NetworkManager,
            GraphicalSession,
        ] {
            assert!(host.has(cap), "{cap}");
        }
        assert_eq!(host.missing(&[KernelErofs, KernelExt4]), [KernelErofs]);
    }

    #[test]
    fn loadable_filesystem_modules_count() {
        let root = FakeRoot::new();
        root.file("proc/filesystems", "\text4\n")
            .file("proc/sys/kernel/osrelease", "7.2.8-test\n")
            .file(
                "lib/modules/7.2.8-test/modules.dep",
                "kernel/fs/overlayfs/overlay.ko.zst:\nkernel/fs/fat/vfat.ko.zst: kernel/fs/fat/fat.ko.zst\n",
            )
            .file("lib/modules/7.2.8-test/modules.builtin", "kernel/fs/btrfs/btrfs.ko\n");
        let host = probe(&root, &[]);
        for cap in [KernelExt4, KernelOverlay, KernelVfat, KernelBtrfs] {
            assert!(host.has(cap), "{cap}");
        }
        assert!(!host.has(KernelErofs));
    }

    #[test]
    fn network_manager_needs_the_system_bus() {
        let root = FakeRoot::new();
        root.dir("run/NetworkManager");
        assert!(!probe(&root, &[]).has(NetworkManager));
        assert!(!probe(&root, &[("DISPLAY", "")]).has(GraphicalSession), "empty is unset");
    }
}
