#![forbid(unsafe_code)]
//! One list of desktop apps (things with a window) for every distro, in the categories app stores use.
//! Command-line tools and services aren't apps: they're packages inside add-ons ([`crate::groups`]).
//! Each app records where it can come from:
//! a native package per distro (and which repo holds it), an AUR package, a Flathub id, and an
//! upstream AppImage for the few apps that ship one and nothing distro-agnostic else.
//!
//! Package names and repo placement were written from knowledge of each distro, not
//! queried live; sizes are rough. A release build should regenerate this table from the
//! distros' package indexes and Flathub's appstream data.

use crate::source::Distro::{self, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Browsers,
    ChatEmail,
    Office,
    AudioVideo,
    Graphics,
    Games,
    Development,
    Utilities,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Browsers,
        Category::ChatEmail,
        Category::Office,
        Category::AudioVideo,
        Category::Graphics,
        Category::Games,
        Category::Development,
        Category::Utilities,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Category::Browsers => "Browsers",
            Category::ChatEmail => "Chat & email",
            Category::Office => "Office",
            Category::AudioVideo => "Audio & video",
            Category::Graphics => "Graphics",
            Category::Games => "Games",
            Category::Development => "Development",
            Category::Utilities => "Utilities",
        }
    }
}

#[derive(Debug)]
pub struct Native {
    pub distro: Distro,
    pub pkg: &'static str,
    pub repo: &'static str,
}

const fn n(distro: Distro, pkg: &'static str, repo: &'static str) -> Native {
    Native { distro, pkg, repo }
}

#[derive(Debug)]
pub struct App {
    pub id: &'static str,
    pub name: &'static str,
    pub what: &'static str,
    pub category: Category,
    /// Rough installed size in MB.
    pub mb: u32,
    pub native: &'static [Native],
    pub aur: Option<&'static str>,
    pub flatpak: Option<&'static str>,
    /// GitHub `owner/repo` whose latest release publishes an x86_64 AppImage. Works on every distro.
    pub appimage: Option<&'static str>,
}

use Category::*;

// Repo ids, shortened for the table below.
const AE: &str = "arch.core-extra";
const AM: &str = "arch.multilib";
const DM: &str = "debian.main";
const DC: &str = "debian.contrib";
const DN: &str = "debian.non-free";
const FF: &str = "fedora.fedora";
const RF: &str = "fedora.rpmfusion-free";
const RN: &str = "fedora.rpmfusion-nonfree";
const OO: &str = "opensuse.oss";
const ON: &str = "opensuse.non-oss";
const OP: &str = "opensuse.packman";

pub const APPS: &[App] = &[
    // Browsers
    App {
        id: "firefox",
        name: "Firefox",
        what: "web browser",
        category: Browsers,
        mb: 280,
        native: &[
            n(Arch, "firefox", AE),
            n(Debian, "firefox-esr", DM),
            n(Fedora, "firefox", FF),
            n(OpenSuse, "MozillaFirefox", OO),
        ],
        aur: None,
        flatpak: Some("org.mozilla.firefox"),
        appimage: None,
    },
    App {
        id: "chromium",
        name: "Chromium",
        what: "web browser",
        category: Browsers,
        mb: 350,
        native: &[
            n(Arch, "chromium", AE),
            n(Debian, "chromium", DM),
            n(Fedora, "chromium", FF),
            n(OpenSuse, "chromium", OO),
        ],
        aur: None,
        flatpak: Some("org.chromium.Chromium"),
        appimage: None,
    },
    App {
        id: "brave",
        name: "Brave",
        what: "private browser",
        category: Browsers,
        mb: 400,
        native: &[],
        aur: Some("brave-bin"),
        flatpak: Some("com.brave.Browser"),
        appimage: None,
    },
    App {
        id: "tor",
        name: "Tor Browser",
        what: "anonymous browsing",
        category: Browsers,
        mb: 300,
        native: &[
            n(Arch, "torbrowser-launcher", AE),
            n(Debian, "torbrowser-launcher", DC),
            n(Fedora, "torbrowser-launcher", FF),
            n(OpenSuse, "torbrowser-launcher", OO),
        ],
        aur: None,
        flatpak: Some("org.torproject.torbrowser-launcher"),
        appimage: None,
    },
    App {
        id: "helium",
        name: "Helium",
        what: "light, private Chromium",
        category: Browsers,
        mb: 320,
        // No Flatpak. Upstream's own apt repo and COPR aren't added; its AppImage runs on every distro.
        native: &[],
        aur: Some("helium-browser-bin"),
        flatpak: None,
        appimage: Some("imputnet/helium-linux"),
    },
    App {
        id: "mullvad",
        name: "Mullvad Browser",
        what: "anti-fingerprinting",
        category: Browsers,
        mb: 300,
        // Flathub build is community-provided; Mullvad's own apt/dnf repos are not added.
        native: &[],
        aur: Some("mullvad-browser-bin"),
        flatpak: Some("net.mullvad.MullvadBrowser"),
        appimage: None,
    },
    // Chat & email
    App {
        id: "thunderbird",
        name: "Thunderbird",
        what: "email",
        category: ChatEmail,
        mb: 280,
        native: &[
            n(Arch, "thunderbird", AE),
            n(Debian, "thunderbird", DM),
            n(Fedora, "thunderbird", FF),
            n(OpenSuse, "MozillaThunderbird", OO),
        ],
        aur: None,
        flatpak: Some("org.mozilla.Thunderbird"),
        appimage: None,
    },
    App {
        id: "discord",
        name: "Discord",
        what: "chat and voice",
        category: ChatEmail,
        mb: 300,
        native: &[n(Arch, "discord", AE), n(Fedora, "discord", RN), n(OpenSuse, "discord", ON)],
        aur: None,
        flatpak: Some("com.discordapp.Discord"),
        appimage: None,
    },
    App {
        id: "signal",
        name: "Signal",
        what: "private messaging",
        category: ChatEmail,
        mb: 300,
        native: &[n(Arch, "signal-desktop", AE)],
        aur: None,
        flatpak: Some("org.signal.Signal"),
        appimage: None,
    },
    App {
        id: "telegram",
        name: "Telegram",
        what: "messaging",
        category: ChatEmail,
        mb: 150,
        native: &[
            n(Arch, "telegram-desktop", AE),
            n(Fedora, "telegram-desktop", RF),
            n(OpenSuse, "telegram-desktop", OO),
        ],
        aur: None,
        flatpak: Some("org.telegram.desktop"),
        appimage: None,
    },
    App {
        id: "element",
        name: "Element",
        what: "Matrix chat",
        category: ChatEmail,
        mb: 250,
        native: &[n(Arch, "element-desktop", AE)],
        aur: None,
        flatpak: Some("im.riot.Riot"),
        appimage: None,
    },
    // Office
    App {
        id: "libreoffice",
        name: "LibreOffice",
        what: "office suite",
        category: Office,
        mb: 650,
        native: &[
            n(Arch, "libreoffice-fresh", AE),
            n(Debian, "libreoffice", DM),
            n(Fedora, "libreoffice", FF),
            n(OpenSuse, "libreoffice", OO),
        ],
        aur: None,
        flatpak: Some("org.libreoffice.LibreOffice"),
        appimage: None,
    },
    App {
        id: "okular",
        name: "Okular",
        what: "PDF reader",
        category: Office,
        mb: 60,
        native: &[
            n(Arch, "okular", AE),
            n(Debian, "okular", DM),
            n(Fedora, "okular", FF),
            n(OpenSuse, "okular", OO),
        ],
        aur: None,
        flatpak: Some("org.kde.okular"),
        appimage: None,
    },
    App {
        id: "obsidian",
        name: "Obsidian",
        what: "notes",
        category: Office,
        mb: 300,
        native: &[n(Arch, "obsidian", AE)],
        aur: None,
        flatpak: Some("md.obsidian.Obsidian"),
        appimage: None,
    },
    App {
        id: "onlyoffice",
        name: "ONLYOFFICE",
        what: "office suite",
        category: Office,
        mb: 700,
        native: &[],
        aur: Some("onlyoffice-bin"),
        flatpak: Some("org.onlyoffice.desktopeditors"),
        appimage: None,
    },
    // Audio & video
    App {
        id: "vlc",
        name: "VLC",
        what: "video player",
        category: AudioVideo,
        mb: 90,
        native: &[
            n(Arch, "vlc", AE),
            n(Debian, "vlc", DM),
            n(Fedora, "vlc", FF),
            n(OpenSuse, "vlc", OO),
        ],
        aur: None,
        flatpak: Some("org.videolan.VLC"),
        appimage: None,
    },
    App {
        id: "mpv",
        name: "mpv",
        what: "minimal player",
        category: AudioVideo,
        mb: 40,
        native: &[
            n(Arch, "mpv", AE),
            n(Debian, "mpv", DM),
            n(Fedora, "mpv", FF),
            n(OpenSuse, "mpv", OO),
        ],
        aur: None,
        flatpak: Some("io.mpv.Mpv"),
        appimage: None,
    },
    App {
        id: "spotify",
        name: "Spotify",
        what: "music streaming",
        category: AudioVideo,
        mb: 300,
        native: &[n(Arch, "spotify-launcher", AE)],
        aur: None,
        flatpak: Some("com.spotify.Client"),
        appimage: None,
    },
    App {
        id: "audacity",
        name: "Audacity",
        what: "audio editing",
        category: AudioVideo,
        mb: 90,
        native: &[
            n(Arch, "audacity", AE),
            n(Debian, "audacity", DM),
            n(Fedora, "audacity", FF),
            n(OpenSuse, "audacity", OO),
        ],
        aur: None,
        flatpak: Some("org.audacityteam.Audacity"),
        appimage: None,
    },
    App {
        id: "codecs",
        name: "Media codecs",
        what: "plays every format",
        category: AudioVideo,
        mb: 40,
        native: &[
            n(Arch, "gst-plugins-ugly", AE),
            n(Debian, "gstreamer1.0-plugins-ugly", DM),
            n(Fedora, "gstreamer1-plugins-ugly", RF),
            n(OpenSuse, "gstreamer-plugins-ugly", OP),
        ],
        aur: None,
        flatpak: None,
        appimage: None,
    },
    // Graphics
    App {
        id: "gimp",
        name: "GIMP",
        what: "photo editing",
        category: Graphics,
        mb: 350,
        native: &[
            n(Arch, "gimp", AE),
            n(Debian, "gimp", DM),
            n(Fedora, "gimp", FF),
            n(OpenSuse, "gimp", OO),
        ],
        aur: None,
        flatpak: Some("org.gimp.GIMP"),
        appimage: None,
    },
    App {
        id: "krita",
        name: "Krita",
        what: "digital painting",
        category: Graphics,
        mb: 420,
        native: &[
            n(Arch, "krita", AE),
            n(Debian, "krita", DM),
            n(Fedora, "krita", FF),
            n(OpenSuse, "krita", OO),
        ],
        aur: None,
        flatpak: Some("org.kde.krita"),
        appimage: None,
    },
    App {
        id: "inkscape",
        name: "Inkscape",
        what: "vector art",
        category: Graphics,
        mb: 230,
        native: &[
            n(Arch, "inkscape", AE),
            n(Debian, "inkscape", DM),
            n(Fedora, "inkscape", FF),
            n(OpenSuse, "inkscape", OO),
        ],
        aur: None,
        flatpak: Some("org.inkscape.Inkscape"),
        appimage: None,
    },
    App {
        id: "blender",
        name: "Blender",
        what: "3D",
        category: Graphics,
        mb: 900,
        native: &[
            n(Arch, "blender", AE),
            n(Debian, "blender", DM),
            n(Fedora, "blender", FF),
            n(OpenSuse, "blender", OO),
        ],
        aur: None,
        flatpak: Some("org.blender.Blender"),
        appimage: None,
    },
    App {
        id: "kdenlive",
        name: "Kdenlive",
        what: "video editing",
        category: AudioVideo,
        mb: 250,
        native: &[
            n(Arch, "kdenlive", AE),
            n(Debian, "kdenlive", DM),
            n(Fedora, "kdenlive", FF),
            n(OpenSuse, "kdenlive", OO),
        ],
        aur: None,
        flatpak: Some("org.kde.kdenlive"),
        appimage: None,
    },
    App {
        id: "obs",
        name: "OBS Studio",
        what: "record and stream",
        category: AudioVideo,
        mb: 220,
        native: &[
            n(Arch, "obs-studio", AE),
            n(Debian, "obs-studio", DM),
            n(Fedora, "obs-studio", FF),
            n(OpenSuse, "obs-studio", OP),
        ],
        aur: None,
        flatpak: Some("com.obsproject.Studio"),
        appimage: None,
    },
    // Games
    App {
        id: "steam",
        name: "Steam",
        what: "game store",
        category: Games,
        mb: 900,
        native: &[
            n(Arch, "steam", AM),
            n(Debian, "steam-installer", DN),
            n(Fedora, "steam", RN),
            n(OpenSuse, "steam", ON),
        ],
        aur: None,
        flatpak: Some("com.valvesoftware.Steam"),
        appimage: None,
    },
    App {
        id: "lutris",
        name: "Lutris",
        what: "game launcher",
        category: Games,
        mb: 60,
        native: &[
            n(Arch, "lutris", AE),
            n(Debian, "lutris", DM),
            n(Fedora, "lutris", FF),
            n(OpenSuse, "lutris", OO),
        ],
        aur: None,
        flatpak: Some("net.lutris.Lutris"),
        appimage: None,
    },
    App {
        id: "heroic",
        name: "Heroic",
        what: "Epic and GOG",
        category: Games,
        mb: 300,
        native: &[],
        aur: Some("heroic-games-launcher-bin"),
        flatpak: Some("com.heroicgameslauncher.hgl"),
        appimage: None,
    },
    App {
        id: "prism",
        name: "Prism Launcher",
        what: "Minecraft",
        category: Games,
        mb: 120,
        native: &[n(Arch, "prismlauncher", AE)],
        aur: None,
        flatpak: Some("org.prismlauncher.PrismLauncher"),
        appimage: None,
    },
    // Development
    App {
        id: "vscodium",
        name: "VSCodium",
        what: "code editor",
        category: Development,
        mb: 380,
        native: &[],
        aur: Some("vscodium-bin"),
        flatpak: Some("com.vscodium.codium"),
        appimage: None,
    },
    App {
        id: "neovim",
        name: "Neovim",
        what: "terminal editor",
        category: Development,
        mb: 30,
        native: &[
            n(Arch, "neovim", AE),
            n(Debian, "neovim", DM),
            n(Fedora, "neovim", FF),
            n(OpenSuse, "neovim", OO),
        ],
        aur: None,
        flatpak: None,
        appimage: None,
    },
    // Utilities
    App {
        id: "keepassxc",
        name: "KeePassXC",
        what: "password manager",
        category: Utilities,
        mb: 40,
        native: &[
            n(Arch, "keepassxc", AE),
            n(Debian, "keepassxc", DM),
            n(Fedora, "keepassxc", FF),
            n(OpenSuse, "keepassxc", OO),
        ],
        aur: None,
        flatpak: Some("org.keepassxc.KeePassXC"),
        appimage: None,
    },
    App {
        id: "syncthing",
        name: "Syncthing",
        what: "file sync",
        category: Utilities,
        mb: 60,
        native: &[
            n(Arch, "syncthing", AE),
            n(Debian, "syncthing", DM),
            n(Fedora, "syncthing", FF),
            n(OpenSuse, "syncthing", OO),
        ],
        aur: None,
        flatpak: None,
        appimage: None,
    },
    App {
        id: "qbittorrent",
        name: "qBittorrent",
        what: "torrents",
        category: Utilities,
        mb: 40,
        native: &[
            n(Arch, "qbittorrent", AE),
            n(Debian, "qbittorrent", DM),
            n(Fedora, "qbittorrent", FF),
            n(OpenSuse, "qbittorrent", OO),
        ],
        aur: None,
        flatpak: Some("org.qbittorrent.qBittorrent"),
        appimage: None,
    },
    App {
        id: "timeshift",
        name: "Timeshift",
        what: "system snapshots",
        category: Utilities,
        mb: 20,
        native: &[n(Arch, "timeshift", AE), n(Debian, "timeshift", DM), n(Fedora, "timeshift", FF)],
        aur: None,
        flatpak: None,
        appimage: None,
    },
    App {
        id: "localsend",
        name: "LocalSend",
        what: "send files to nearby devices",
        category: Utilities,
        mb: 60,
        // Not in any distro repo; offered inside the File Sharing add-on rather than under Applications.
        native: &[],
        aur: Some("localsend-bin"),
        flatpak: Some("org.localsend.localsend_app"),
        appimage: Some("localsend/localsend"),
    },
    App {
        id: "flatseal",
        name: "Flatseal",
        what: "Flatpak permissions",
        category: Utilities,
        mb: 10,
        native: &[],
        aur: None,
        flatpak: Some("com.github.tchx84.Flatseal"),
        appimage: None,
    },
];

/// In the catalogue and found by search, but not listed in the category grid.
pub const SEARCH_ONLY: &[&str] = &["brave", "tor"];

pub fn listed(a: &App) -> bool {
    !SEARCH_ONLY.contains(&a.id)
}

pub fn app(id: &str) -> Option<&'static App> {
    APPS.iter().find(|a| a.id == id)
}

pub fn in_category(c: Category) -> impl Iterator<Item = &'static App> {
    APPS.iter().filter(move |a| a.category == c)
}

/// Quick picks shown at the top of the rail, like WinUtil's presets.
/// Quick picks: desktop apps, plus the add-ons that belong with them (see [`crate::groups`]).
pub const PRESETS: &[(&str, &[&str], &[&str])] = &[
    (
        "Essentials",
        &["firefox", "thunderbird", "libreoffice", "okular", "vlc", "codecs", "keepassxc"],
        &[],
    ),
    ("Gaming", &["steam", "lutris", "heroic", "prism", "discord"], &["gaming"]),
    ("Creator", &["gimp", "krita", "inkscape", "kdenlive", "obs", "audacity"], &[]),
    ("Developer", &["vscodium", "neovim"], &["dev", "containers"]),
];
