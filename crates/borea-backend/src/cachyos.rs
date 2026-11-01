//! CachyOS minimal overlay over an authenticated Arch tool root.
//! Keyring: CachyOS/CachyOS-PKGBUILDS @ 4dd0b9d487fa3dc2975c0acd155a93c974176783.
//! Master fingerprint: 882DCFE48E2051D48E2562ABF3B607488DB35A47.
pub const KEYRING: &[u8] = include_bytes!("../data/cachyos/cachyos.gpg");
pub const TRUSTED: &str = include_str!("../data/cachyos/cachyos-trusted");
pub const REVOKED: &str = include_str!("../data/cachyos/cachyos-revoked");
pub const MIRRORLIST: &str = "Server = https://cdn77.cachyos.org/repo/$arch/$repo\nServer = https://us.cachyos.org/repo/$arch/$repo\nServer = https://mirror.lesviallon.fr/cachy/repo/$arch/$repo\n";
/// The installer's tool root runs Arch's pacman, which doesn't expand CachyOS's
/// $arch_v3, so the path is spelled out here. The installed system gets CachyOS's
/// own pacman and its signed cachyos-v3-mirrorlist package.
pub const V3_MIRRORLIST: &str = "Server = https://cdn77.cachyos.org/repo/x86_64_v3/$repo\nServer = https://us.cachyos.org/repo/x86_64_v3/$repo\nServer = https://mirror.lesviallon.fr/cachy/repo/x86_64_v3/$repo\n";
/// Present in a review when the CPU takes CachyOS's x86-64-v3 repositories.
pub const V3: &str = "cachyos-v3-mirrorlist";
pub const PACKAGES: &[&str] = &[
    "cachyos-keyring",
    "cachyos-mirrorlist",
    "cachyos-settings",
    "cachyos-hooks",
    "linux-cachyos",
    "linux-cachyos-headers",
];
/// CachyOS's repositories ahead of Arch's, as CachyOS orders them: on an
/// x86-64-v3 CPU its optimised v3, core and extra builds first. Packages that
/// share a name then come from CachyOS; Arch's fill the rest.
pub fn repositories(v3: bool) -> String {
    let optimised = if v3 {
        ["cachyos-v3", "cachyos-core-v3", "cachyos-extra-v3"]
            .map(|repo| format!("[{repo}]\nInclude = /etc/pacman.d/cachyos-v3-mirrorlist\n"))
            .concat()
    } else {
        String::new()
    };
    format!("{optimised}[cachyos]\nInclude = /etc/pacman.d/cachyos-mirrorlist\n")
}
/// pacman's Architecture line: v3 packages are built for x86_64_v3.
pub fn architecture(v3: bool) -> &'static str {
    if v3 { "x86_64 x86_64_v3" } else { "auto" }
}
/// Whether this review installs CachyOS's x86-64-v3 builds.
pub fn optimised(packages: &[borea_core::Package]) -> bool {
    packages.iter().any(|p| p.name == V3)
}
