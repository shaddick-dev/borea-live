//! CachyOS minimal overlay over an authenticated Arch tool root.
//! Keyring: CachyOS/CachyOS-PKGBUILDS @ 4dd0b9d487fa3dc2975c0acd155a93c974176783.
//! Master fingerprint: 882DCFE48E2051D48E2562ABF3B607488DB35A47.
pub const KEYRING: &[u8] = include_bytes!("../data/cachyos/cachyos.gpg");
pub const TRUSTED: &str = include_str!("../data/cachyos/cachyos-trusted");
pub const REVOKED: &str = include_str!("../data/cachyos/cachyos-revoked");
pub const MIRRORLIST: &str = "Server = https://cdn77.cachyos.org/repo/$arch/$repo\nServer = https://us.cachyos.org/repo/$arch/$repo\n";
pub const PACKAGES: &[&str] = &[
    "cachyos-keyring",
    "cachyos-mirrorlist",
    "cachyos-settings",
    "cachyos-hooks",
    "linux-cachyos",
    "linux-cachyos-headers",
];
pub fn repositories() -> &'static str {
    "[cachyos]\nInclude = /etc/pacman.d/cachyos-mirrorlist\n"
}
