#![forbid(unsafe_code)]
//! Traditional install groups, defined once and mapped onto every distro.
//!
//! * One **base environment** (Minimal, Server, Server with GUI, Workstation).
//! * Any number of **add-ons**: real-world roles in three sections — server roles, virtualization,
//!   tools — modelled on Fedora/RHEL comps groups.
//!
//! Every package in a group can be ticked or unticked ([`Picks`]), except the few a bootable
//! system needs ([`REQUIRED`]). Each group lists its default packages (`items`), packages only
//! added when there's a desktop (`gui_items`), and extras that start unticked (`optional`).
//!
//! Groups hold command-line tools and services. Desktop apps (things with a window) live in the
//! app catalogue ([`crate::catalog`]); nothing appears in both.
//!
//! Package keys are distro-neutral ([`PKGS`]) and name the real package on each distro; all live in
//! the distros' base repos. Names were written from knowledge of each distro: check against the
//! package indexes before release.

use crate::source::Distro::{self, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Environment,
    AddOn,
}

#[derive(Debug)]
pub struct Group {
    pub id: &'static str,
    pub name: &'static str,
    pub what: &'static str,
    pub kind: Kind,
    /// Add-ons only: the heading it's listed under.
    pub section: &'static str,
    /// Installs the desktop chosen in step 04 (and a display manager).
    pub desktop: bool,
    /// Desktop apps from the catalogue only make sense with a desktop.
    pub allows_apps: bool,
    pub items: &'static [&'static str],
    /// Only when the environment has a desktop (e.g. virt-manager).
    pub gui_items: &'static [&'static str],
    /// Offered, unticked by default.
    pub optional: &'static [&'static str],
    /// Desktop apps from the catalogue that belong to this role (ticked by default, desktop only).
    /// They resolve through the normal sources (repos, Flathub…) and aren't listed under Applications.
    pub apps: &'static [&'static str],
}

/// A system that boots and gets online. Shown ticked and locked.
pub const REQUIRED: &[&str] = &["base", "kernel", "firmware", "networkmanager", "sudo"];

pub const SECTIONS: &[&str] = &["Server roles", "Virtualization", "Tools"];

const fn env(
    id: &'static str,
    name: &'static str,
    what: &'static str,
    desktop: bool,
    items: &'static [&'static str],
    optional: &'static [&'static str],
) -> Group {
    Group {
        id,
        name,
        what,
        kind: Kind::Environment,
        section: "",
        desktop,
        allows_apps: desktop,
        items,
        gui_items: &[],
        optional,
        apps: &[],
    }
}

const fn addon(
    id: &'static str,
    section: &'static str,
    name: &'static str,
    what: &'static str,
    items: &'static [&'static str],
    gui_items: &'static [&'static str],
    optional: &'static [&'static str],
) -> Group {
    Group {
        id,
        name,
        what,
        kind: Kind::AddOn,
        section,
        desktop: false,
        allows_apps: true,
        items,
        gui_items,
        optional,
        apps: &[],
    }
}

pub const GROUPS: &[Group] = &[
    env(
        "minimal",
        "Minimal install",
        "just enough to boot, log in and get online",
        false,
        &["base", "kernel", "firmware", "networkmanager", "sudo", "nano"],
        &["openssh"],
    ),
    env(
        "server",
        "Server",
        "headless, managed over SSH or the web",
        false,
        &[
            "base",
            "kernel",
            "firmware",
            "networkmanager",
            "sudo",
            "nano",
            "openssh",
            "cockpit",
            "firewalld",
            "chrony",
            "fail2ban",
            "rsync",
            "tmux",
        ],
        &[],
    ),
    env(
        "server-gui",
        "Server with GUI",
        "a full server, plus your desktop to manage it",
        true,
        &[
            "base",
            "kernel",
            "firmware",
            "networkmanager",
            "sudo",
            "nano",
            "openssh",
            "cockpit",
            "firewalld",
            "chrony",
            "fail2ban",
            "rsync",
            "tmux",
            "pipewire",
        ],
        &[],
    ),
    env(
        "workstation",
        "Workstation",
        "a desktop for everyday use: sound, printing, Bluetooth",
        true,
        &[
            "base",
            "kernel",
            "firmware",
            "networkmanager",
            "sudo",
            "nano",
            "pipewire",
            "cups",
            "bluez",
            "firewalld",
        ],
        &["openssh"],
    ),
    addon(
        "web",
        "Server roles",
        "Web Server",
        "nginx with TLS certificates",
        &["nginx", "certbot"],
        &[],
        &[],
    ),
    addon(
        "database",
        "Server roles",
        "Database Server",
        "PostgreSQL, or MariaDB",
        &["postgresql"],
        &[],
        &["mariadb"],
    ),
    Group {
        apps: &["localsend"],
        ..addon(
            "files",
            "Server roles",
            "File Sharing",
            "share folders and send files across your network",
            &["samba"],
            &[],
            &["nfs-server"],
        )
    },
    addon(
        "virt",
        "Virtualization",
        "Virtualization Host",
        "run virtual machines with KVM",
        &["qemu", "libvirt", "ovmf", "dnsmasq"],
        &["virt-manager"],
        &["cockpit-machines"],
    ),
    addon(
        "containers",
        "Virtualization",
        "Container Management",
        "build and run containers",
        &["podman", "buildah", "skopeo", "distrobox"],
        &[],
        &["cockpit-podman"],
    ),
    addon(
        "dev",
        "Tools",
        "Development Tools",
        "compilers, git and debuggers",
        &["build", "make", "git", "gdb", "cmake", "python"],
        &[],
        &["rustup"],
    ),
    addon(
        "monitor",
        "Tools",
        "Monitoring",
        "system load, disks and temperatures",
        &["btop", "smartmontools", "lm-sensors", "iotop"],
        &[],
        &[],
    ),
    addon(
        "network",
        "Tools",
        "Network Tools",
        "scan, trace and measure networks",
        &["nmap", "tcpdump", "mtr", "iperf3"],
        &[],
        &["tshark"],
    ),
    addon(
        "gaming",
        "Tools",
        "Gaming Tweaks",
        "performance mode and an FPS overlay",
        &["gamemode", "mangohud"],
        &[],
        &[],
    ),
];

/// Distro-neutral key → package name per distro (Arch, Debian, Fedora, openSUSE).
pub const PKGS: &[(&str, [&str; 4])] = &[
    ("base", ["base", "systemd-sysv", "systemd", "systemd"]),
    ("kernel", ["linux", "linux-image-amd64", "kernel", "kernel-default"]),
    ("firmware", ["linux-firmware", "firmware-linux", "linux-firmware", "kernel-firmware-all"]),
    ("networkmanager", ["networkmanager", "network-manager", "NetworkManager", "NetworkManager"]),
    ("sudo", ["sudo", "sudo", "sudo", "sudo"]),
    ("nano", ["nano", "nano", "nano", "nano"]),
    ("openssh", ["openssh", "openssh-server", "openssh-server", "openssh-server"]),
    ("cockpit", ["cockpit", "cockpit", "cockpit", "cockpit"]),
    ("firewalld", ["firewalld", "firewalld", "firewalld", "firewalld"]),
    ("chrony", ["chrony", "chrony", "chrony", "chrony"]),
    ("fail2ban", ["fail2ban", "fail2ban", "fail2ban", "fail2ban"]),
    ("rsync", ["rsync", "rsync", "rsync", "rsync"]),
    ("tmux", ["tmux", "tmux", "tmux", "tmux"]),
    ("pipewire", ["pipewire", "pipewire-audio", "pipewire", "pipewire"]),
    ("cups", ["cups", "cups", "cups", "cups"]),
    ("bluez", ["bluez", "bluez", "bluez", "bluez"]),
    ("nginx", ["nginx", "nginx", "nginx", "nginx"]),
    ("certbot", ["certbot", "certbot", "certbot", "python3-certbot"]),
    ("postgresql", ["postgresql", "postgresql", "postgresql-server", "postgresql-server"]),
    ("mariadb", ["mariadb", "mariadb-server", "mariadb-server", "mariadb"]),
    ("samba", ["samba", "samba", "samba", "samba"]),
    ("nfs-server", ["nfs-utils", "nfs-kernel-server", "nfs-utils", "nfs-kernel-server"]),
    ("qemu", ["qemu-desktop", "qemu-system-x86", "qemu-kvm", "qemu-kvm"]),
    ("libvirt", ["libvirt", "libvirt-daemon-system", "libvirt", "libvirt"]),
    ("ovmf", ["edk2-ovmf", "ovmf", "edk2-ovmf", "qemu-ovmf-x86_64"]),
    ("dnsmasq", ["dnsmasq", "dnsmasq", "dnsmasq", "dnsmasq"]),
    ("virt-manager", ["virt-manager", "virt-manager", "virt-manager", "virt-manager"]),
    (
        "cockpit-machines",
        ["cockpit-machines", "cockpit-machines", "cockpit-machines", "cockpit-machines"],
    ),
    ("podman", ["podman", "podman", "podman", "podman"]),
    ("buildah", ["buildah", "buildah", "buildah", "buildah"]),
    ("skopeo", ["skopeo", "skopeo", "skopeo", "skopeo"]),
    ("distrobox", ["distrobox", "distrobox", "distrobox", "distrobox"]),
    ("cockpit-podman", ["cockpit-podman", "cockpit-podman", "cockpit-podman", "cockpit-podman"]),
    ("build", ["base-devel", "build-essential", "gcc", "gcc"]),
    ("make", ["make", "make", "make", "make"]),
    ("git", ["git", "git", "git", "git"]),
    ("gdb", ["gdb", "gdb", "gdb", "gdb"]),
    ("cmake", ["cmake", "cmake", "cmake", "cmake"]),
    ("python", ["python", "python3", "python3", "python3"]),
    ("rustup", ["rustup", "rustup", "rustup", "rustup"]),
    ("btop", ["btop", "btop", "btop", "btop"]),
    ("smartmontools", ["smartmontools", "smartmontools", "smartmontools", "smartmontools"]),
    ("lm-sensors", ["lm_sensors", "lm-sensors", "lm_sensors", "sensors"]),
    ("iotop", ["iotop", "iotop", "iotop-c", "iotop"]),
    ("nmap", ["nmap", "nmap", "nmap", "nmap"]),
    ("tcpdump", ["tcpdump", "tcpdump", "tcpdump", "tcpdump"]),
    ("mtr", ["mtr", "mtr-tiny", "mtr", "mtr"]),
    ("iperf3", ["iperf3", "iperf3", "iperf3", "iperf3"]),
    ("tshark", ["wireshark-cli", "tshark", "wireshark-cli", "wireshark"]),
    ("gamemode", ["gamemode", "gamemode", "gamemode", "gamemode"]),
    ("mangohud", ["mangohud", "mangohud", "mangohud", "mangohud"]),
];

/// What each package or app needs opened in firewalld (service names, or port/protocol).
pub const FIREWALL: &[(&str, &[&str])] = &[
    ("openssh", &["ssh"]),
    ("cockpit", &["cockpit"]),
    ("nginx", &["http", "https"]),
    ("samba", &["samba"]),
    ("nfs-server", &["nfs"]),
    ("localsend", &["53317/tcp", "53317/udp"]),
];

/// Services provided by selected system roles, in Arch/Debian/Fedora/openSUSE order.
pub const SERVICES: &[(&str, [&[&str]; 4])] = &[
    (
        "firewalld",
        [
            &["firewalld.service"],
            &["firewalld.service"],
            &["firewalld.service"],
            &["firewalld.service"],
        ],
    ),
    ("openssh", [&["sshd.service"], &["ssh.service"], &["sshd.service"], &["sshd.service"]]),
    (
        "cockpit",
        [&["cockpit.socket"], &["cockpit.socket"], &["cockpit.socket"], &["cockpit.socket"]],
    ),
    (
        "chrony",
        [&["chronyd.service"], &["chrony.service"], &["chronyd.service"], &["chronyd.service"]],
    ),
    ("cups", [&["cups.service"], &["cups.service"], &["cups.service"], &["cups.service"]]),
    (
        "bluez",
        [
            &["bluetooth.service"],
            &["bluetooth.service"],
            &["bluetooth.service"],
            &["bluetooth.service"],
        ],
    ),
    ("nginx", [&["nginx.service"], &["nginx.service"], &["nginx.service"], &["nginx.service"]]),
    (
        "postgresql",
        [
            &["postgresql.service"],
            &["postgresql.service"],
            &["postgresql.service"],
            &["postgresql.service"],
        ],
    ),
    (
        "mariadb",
        [&["mariadb.service"], &["mariadb.service"], &["mariadb.service"], &["mariadb.service"]],
    ),
    (
        "samba",
        [
            &["smb.service", "nmb.service"],
            &["smbd.service", "nmbd.service"],
            &["smb.service", "nmb.service"],
            &["smb.service", "nmb.service"],
        ],
    ),
    (
        "nfs-server",
        [
            &["nfs-server.service"],
            &["nfs-kernel-server.service"],
            &["nfs-server.service"],
            &["nfs-server.service"],
        ],
    ),
    (
        "libvirt",
        [
            &["libvirtd.socket"],
            &["libvirtd.socket"],
            &["virtqemud.socket", "virtnetworkd.socket", "virtstoraged.socket"],
            &["libvirtd.socket"],
        ],
    ),
];
pub fn role_services(distro: Distro, packages: &[&str]) -> Vec<&'static str> {
    SERVICES
        .iter()
        .filter(|(key, _)| package(key, distro).is_some_and(|name| packages.contains(&name)))
        .flat_map(|(_, services)| services[idx(distro)].iter().copied())
        .collect()
}

/// Which add-on (if any) offers this catalogue app. Such apps aren't listed under Applications.
pub fn addon_for_app(app_id: &str) -> Option<&'static Group> {
    GROUPS.iter().find(|g| g.apps.contains(&app_id))
}

/// Desktop (from step 04) → (packages, display manager) per distro.
pub const DESKTOPS: &[(&str, [(&str, &str); 4])] = &[
    (
        "plasma",
        [
            ("plasma-meta", "sddm"),
            ("task-kde-desktop", "sddm"),
            ("@kde-desktop-environment", "sddm"),
            ("patterns-kde-kde_plasma", "sddm"),
        ],
    ),
    (
        "gnome",
        [
            ("gnome", "gdm"),
            ("task-gnome-desktop", "gdm3"),
            ("@workstation-product-environment", "gdm"),
            ("patterns-gnome-gnome", "gdm"),
        ],
    ),
    (
        "cosmic",
        [
            ("cosmic", "cosmic-greeter"),
            ("cosmic-session", "cosmic-greeter"),
            ("@cosmic-desktop-environment", "cosmic-greeter"),
            ("patterns-cosmic-cosmic", "cosmic-greeter"),
        ],
    ),
    (
        "budgie",
        [
            ("budgie", "lightdm"),
            ("budgie-desktop", "lightdm"),
            ("@budgie-desktop-environment", "lightdm"),
            ("patterns-budgie-budgie", "lightdm"),
        ],
    ),
];

/// Verified upstream omissions: no vendor repository is substituted silently.
pub const UNAVAILABLE_DESKTOPS: &[(Distro, &str, &str)] = &[(
    Debian,
    "cosmic",
    "COSMIC is not packaged in Debian 13's official repositories; choose another desktop.",
)];
pub fn desktop_unavailable(distro: Distro, desktop: &str) -> Option<&'static str> {
    UNAVAILABLE_DESKTOPS
        .iter()
        .find(|(d, id, _)| *d == distro && *id == desktop)
        .map(|(_, _, reason)| *reason)
}

fn idx(d: Distro) -> usize {
    match d {
        Arch => 0,
        Debian => 1,
        Fedora => 2,
        OpenSuse => 3,
    }
}

pub fn group(id: &str) -> Option<&'static Group> {
    GROUPS.iter().find(|g| g.id == id)
}

pub fn package(key: &str, d: Distro) -> Option<&'static str> {
    PKGS.iter().find(|(k, _)| *k == key).map(|(_, names)| names[idx(d)])
}

impl Group {
    /// Every package this group offers, in display order.
    /// Every package (and, with a desktop, every app) this group offers, in display order.
    pub fn offers(&self, with_desktop: bool) -> impl Iterator<Item = &'static str> + '_ {
        let gui: &'static [&'static str] = if with_desktop { self.gui_items } else { &[] };
        let apps: &'static [&'static str] = if with_desktop { self.apps } else { &[] };
        self.items.iter().chain(gui).chain(apps).chain(self.optional).copied()
    }

    pub fn default_on(&self, key: &str, with_desktop: bool) -> bool {
        self.items.contains(&key)
            || (with_desktop && (self.gui_items.contains(&key) || self.apps.contains(&key)))
    }
}

/// The user's ticks and unticks inside groups. Anything not mentioned keeps its default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picks {
    set: Vec<(&'static str, &'static str, bool)>,
}

impl Picks {
    pub fn set(&mut self, group_id: &str, key: &str, on: bool) -> Result<(), GroupError> {
        let g = group(group_id).ok_or_else(|| GroupError::UnknownGroup(group_id.into()))?;
        let key =
            g.offers(true).find(|k| *k == key).ok_or_else(|| GroupError::NotInGroup(key.into()))?;
        if !on && g.kind == Kind::Environment && REQUIRED.contains(&key) {
            return Err(GroupError::Required(key));
        }
        self.set.retain(|(gid, k, _)| !(*gid == g.id && *k == key));
        self.set.push((g.id, key, on));
        Ok(())
    }

    pub fn is_on(&self, g: &Group, key: &str, with_desktop: bool) -> bool {
        self.set
            .iter()
            .rev()
            .find(|(gid, k, _)| *gid == g.id && *k == key)
            .map(|x| x.2)
            .unwrap_or_else(|| g.default_on(key, with_desktop))
    }
}

/// What the chosen environment + add-ons install on this distro, de-duplicated, in order.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct GroupPlan {
    pub packages: Vec<&'static str>,
    /// Catalogue apps the add-ons bring (resolve them like any picked app).
    pub apps: Vec<&'static str>,
    /// firewalld services/ports to open for what's installed (when firewalld is part of the system).
    pub firewall: Vec<&'static str>,
    pub desktop: Option<(&'static str, &'static str)>,
    /// Desktop apps from the catalogue are skipped (no desktop to run them).
    pub apps_allowed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GroupError {
    NotAnEnvironment(String),
    NotAnAddOn(String),
    UnknownDesktop(String),
    UnavailableDesktop(&'static str),
    UnknownGroup(String),
    NotInGroup(String),
    /// Can't untick what the system needs to boot.
    Required(&'static str),
}

pub fn plan_groups(
    env: &str,
    addons: &[&str],
    desktop: &str,
    d: Distro,
) -> Result<GroupPlan, GroupError> {
    plan_groups_with(env, addons, &Picks::default(), desktop, d)
}

pub fn plan_groups_with(
    env: &str,
    addons: &[&str],
    picks: &Picks,
    desktop: &str,
    d: Distro,
) -> Result<GroupPlan, GroupError> {
    let e = group(env)
        .filter(|g| g.kind == Kind::Environment)
        .ok_or_else(|| GroupError::NotAnEnvironment(env.into()))?;
    let mut keys: Vec<&'static str> = Vec::new();
    let mut take = |g: &Group| {
        for k in g.offers(e.desktop) {
            if (REQUIRED.contains(&k) && g.kind == Kind::Environment
                || picks.is_on(g, k, e.desktop))
                && !keys.contains(&k)
            {
                keys.push(k);
            }
        }
    };
    take(e);
    for a in addons {
        take(
            group(a)
                .filter(|g| g.kind == Kind::AddOn)
                .ok_or_else(|| GroupError::NotAnAddOn((*a).into()))?,
        );
    }
    let desktop = if e.desktop {
        if let Some(reason) = desktop_unavailable(d, desktop) {
            return Err(GroupError::UnavailableDesktop(reason));
        }
        let (_, per) = DESKTOPS
            .iter()
            .find(|(id, _)| *id == desktop)
            .ok_or_else(|| GroupError::UnknownDesktop(desktop.into()))?;
        Some(per[idx(d)])
    } else {
        None
    };
    let packages = keys.iter().filter_map(|k| package(k, d)).collect();
    let apps = keys.iter().copied().filter(|k| package(k, d).is_none()).collect();
    let firewall = if keys.contains(&"firewalld") {
        keys.iter()
            .filter_map(|k| FIREWALL.iter().find(|(f, _)| f == k))
            .flat_map(|(_, rules)| rules.iter().copied())
            .collect()
    } else {
        Vec::new()
    };
    Ok(GroupPlan { packages, apps, firewall, desktop, apps_allowed: e.allows_apps })
}
