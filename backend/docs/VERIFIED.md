# Backend verification evidence

Date: 2026-10-27. This is an evidence ledger, not a completion certificate.

## Passing checks

- Firmware footer update: all 87 backend tests and all 44 native GUI tests pass on stable; the 18 tests in the changed backend packages also pass on Rust 1.85. Formatting and Clippy with warnings denied pass for the backend workspace and GUI. Tests cover firmware-variable decoding, reboot refusal during installation, confirmation/cancellation, disabled legacy-BIOS shortcut, preview isolation, and explicit Secure Boot-off plans for all four base distributions. These checks do not reboot physical firmware.
- Workspace compilation, formatting, all 84 default unit/integration tests and Clippy with warnings denied pass under Rust 1.85 and stable. The static musl helper also builds under Rust 1.85. GitHub CI independently passes both Rust 1.85 and stable at `15f6f8b` ([run](https://github.com/shaddick-dev/shaddick.dev/actions/runs/40448714631)).
- Native daemon/CLI Unix-socket smoke test passes in a disposable container: live-user access, unrelated-user refusal, directory/socket ownership and modes, and valid initial event polling. [Results](verification/service-socket-2026-10-27.txt).
- All 84 default workspace unit/integration tests pass, including catalog rules, validation/navigation, locale, layout, journal consistency, EFI preservation, power, executor redaction, mount cleanup, stdin, console markers, static-helper, native audit and service protocol tests. The native NTFS fixture is separately invoked.
- Pure job matrix: 120 combinations, four distributions × workstation/Plasma, server, minimal × whole/alongside × five hardware fixtures. Jobs serialize round-trip and retain the Review package selection; destructive and secret commands are checked. The 240 checked-in Review/script snapshots make changes reviewable. Rendered scripts exit before execution; only the approved service executes jobs.
- 512-byte and 4096-byte sector geometry; refusal of live/mounted/read-only/replaced disks and unsafe NTFS.
- Generated locale data: 327 UTF-8 locales, 590 keyboard layout/variant entries, 485 time zones, 247 country suggestions. Every country suggestion validates. Host-specific `localtime` and `posixrules` are excluded. Generation provenance is in `LOCALE-DATA.md`.
- Official public release keys, fingerprints and origin digests are recorded in `../dist/keys/PROVENANCE.md` and `fingerprints.txt`.
- Arch native metadata audit: all 122 catalog/desktop/add-on/driver/helper entries pass against current signed core/extra/multilib metadata. [Results](verification/arch-native-2026-10-27.txt).
- Fedora's complete native audit passes after replacing obsolete `iotop` with `iotop-c`. GitHub's official-container audit independently passes Arch, Debian, Fedora and all external sources; the later corrected openSUSE audit also passes. [CI evidence](verification/github-package-audit-2026-10-27.json).
- Debian's expanded 114-entry native audit passes, including declared components and i386 NVIDIA libraries. It caught two component errors, Lutris and `nvidia-open-kernel-dkms`; both actually belong to contrib and are corrected. [Results](verification/debian-native-2026-10-27.txt). Native Steam enables contrib/i386, and signed debootstrap preflight resolves its dependencies. Telegram uses Flathub because `telegram-desktop` is absent. Debian COSMIC is explicitly unavailable under the native-source policy: `cosmic-session` and `cosmic-greeter` are absent. The researched community installer converts Fedora COPR RPMs with `alien`; it was not executed or treated as native Debian support.
- Flathub public metadata audit: all 32 enabled app IDs match maintained x86-64 bundles. The end-of-life `org.mozilla.Thunderbird` candidate was removed; native Thunderbird remains available on all four distros. The audit checks metadata availability; native Flatpak authenticates repository/payload signatures during installation. Per-entry [Debian](verification/debian-native-2026-10-26.txt) and [Flathub](verification/flathub-2026-10-26.txt) results are saved.
- The separately invoked native NTFS integration test passes against a freshly formatted 256 MiB regular image using Debian's actual ntfsprogs. Absent, inactive and active hibernation states are detected, with identical full-image SHA-256 before and after every probe. This caught the older ntfsinfo output difference and validates the erased-journal fallback. Pure tests check dirty, newest and torn restart pages; a real Windows journal matrix remains outstanding.
- AUR, Flathub and upstream AppImage validation: all 44 external entries pass (10 AUR, 32 Flathub, two AppImage releases). AppImage checks include actual release/checksum retrieval. [Results](verification/external-2026-10-27.txt).
- Native GPT-file resize test passes: `sfdisk -N` with size on stdin preserves the partition GUID, type, start, attributes and name. The fresh alongside VM also completed NTFS and GPT shrinking and proceeded to encrypted installation.
- Fresh Debian whole-drive install and automatic subsequent boot pass on GitHub, without repair, at `66176de`: encrypted-root unlock, distribution, account, networking, sudo and shutdown assertions. [CI evidence](verification/github-whole-drive-2026-10-27.json).
- The earlier local Debian whole-drive QEMU installation reached encrypted native package installation but failed at final update-grub because `/boot/grub` was absent. Production now creates it. A separately marked diagnostic repair regenerated GRUB; the subsequent automatic boot test passes distribution, account, LUKS, networking, sudo and shutdown assertions. This is a [repaired boot pass](verification/debian-repaired-boot-2026-10-27.txt), not an uninterrupted installation pass.
- Native Debian package inspection confirms the signed shim/MOK-manager paths and cryptroot's `initramfs` crypttab option, needed when the live mapping name differs from the installed mapping. Upstream `sdbootutil` and `mokutil` source inspection informed their native CLI and secret-stdin handling.

- Tumbleweed native catalog audit passes at `bf7841d`, including official/NVIDIA/Packman repository setup, native capabilities and the three corrected package names. [CI evidence](verification/github-tumbleweed-audit-2026-10-27.json).

- A fresh nightly audit independently passes all four native distro inventories and external sources at `64f1140`. [Run](https://github.com/shaddick-dev/shaddick.dev/actions/runs/40484473113).

- Fresh Debian Secure Boot/alongside install and automatic boot pass end to end at `cc6b670`, without repair: original NTFS UUID/data, approved resized GPT geometry, both EFI sentinels, encrypted-root unlock, account, network, sudo and enforced firmware policy. [CI evidence](verification/debian-clean-secure-alongside-2026-10-27.json).

## Checks in progress / release gates

- The firmware setup shortcut needs a real-machine reboot check. It requires firmware advertising OS-to-setup support; unsupported machines use their firmware startup key. Secure Boot-off installation remains supported on UEFI; legacy BIOS target boot is not implemented.

- Arch installs and boots to the selected user; full console assertions confirm distribution, LUKS, network and sudo. The harness missed the sudo marker because of OSC 3008 terminal controls and timed out before shutdown. Console parsing is corrected; full clean rerun pending. [Evidence](verification/arch-installed-assertions-2026-10-27.txt).
- Fedora now passes signed discovery, preflight, installation and account configuration. DNF5 output uses explicit newlines. Kernel installation uses native BOOT_ROOT=/boot for GRUB BLS entries. A completed install reached boot but unlabeled files left PID1 in kernel_t under enforcing SELinux. Native offline setfiles labeling is now included; enforcing-state boot assertion and rerun pending.
- Tumbleweed now passes certificate handling, filesystem setup and package installation. Its minimal pattern omits useradd and wheel. Native shadow and util-linux service units are explicit, using the published systemd package, and the administrator group is created before the account. Account/boot rerun pending.
- Cloud-local Fedora mirror redirection, Tumbleweed CDN metadata and NVIDIA's repository have returned proxy CONNECT 403 denials. Remote CI can reach these upstreams; no unsigned substitute is used.
- Arch/Fedora/Tumbleweed complete native install and boot runs. The nightly `backend-vm.yml` workflow exercises whole-drive Secure Boot-off installs for all four bases; manual selection expands to both layouts and firmware policies. A workflow definition is not a passing result.
- Whole/alongside × Secure Boot off/on QEMU matrix for every distro, real Windows boot preservation, NVIDIA module signing/MOK and real Wi-Fi hardware activation/carry-over. The harness now supports synthetic NTFS/EFI preservation and Microsoft-enrolled firmware scenarios with a genuine signed live boot chain; implemented entry points do not count as passing installed-system results. Arch owner-key enrollment requires separate Setup Mode automation.
- Native package versions and transitive dependencies remain resolved by signed upstream metadata at execution; this is not a full reproducible package lockfile.

Native metadata preflight, approval checks and disk guards are enforced in code. Missing release evidence is not bypassed by calling an adapter complete. The old preview GUI is not enabled for destructive installs by this change.

## Arch / Noctalia pathway — 30 October 2026

- [Official mirror database audit](verification/arch-noctalia-2026-10-30.json): 36/36 desktop, Arc graphics and optional-extra package names found. Metadata only; not installed-system verification.
- Noctalia upstream is 5.4.1; Arch currently provides 5.4.0-1. Latest-v5 installation is blocked before storage writes until the signed package catches up.
- New unit/integration tests cover both storage plans, the GUI approval/start contract and first-login queue behaviour. No claim of successful real graphical login, lock/unlock, suspend/resume or Arc A770 hardware testing.
- The live Rust `--check-noctalia` request received HTTP 503 in this cloud environment. The version comparison is covered by automated tests; upstream release and mirror versions were independently read with the environment's HTTP client. Full online approval is not claimed as verified here.


## Noctalia compositor paths — 31 October 2026

- Hyprland, niri and MangoWC now have separate native recipes on Arch and CachyOS Minimal. Tests cover all six combinations × both storage layouts, region propagation, graphical greeter, desktop serialization and quiet optional-app deferral.
- Generated configs passed the real signed Arch compositor binaries: Hyprland 0.57.0 `--verify-config`, niri 26.04 `validate`, MangoWC 0.18.4 `-p`, as UID 65534 in a disposable authenticated Arch bootstrap. CI repeats native parser checks. [Package and validation record](verification/noctalia-compositors-2026-10-31.json).
- Native parsing caught and fixed a missing KDL terminator. Parsing is not session startup certification. MangoWC's headless launch stopped at renderer creation because scenefx requires a DRM FD.
- Noctalia 5.4.1 remains ahead of the signed 5.4.0-1 package. The existing version gate covers all three recipes and blocks before target writes. Full installed graphical login, lock/unlock, suspend and Arc A770 testing remain outstanding. CachyOS branded preset integration is still a preview.

## Noctalia native-package unblock — 31 October 2026

- The earlier exact GitHub-patch gate is superseded: Review now approves stable major v5, and native pacman preparation accepts signed stable 5.4.0+ packages before target writes. Exact-version approvals from older jobs remain exact. Tests reject v4/v6, old config APIs, development versions and malformed versions.
- Earlier mirror metadata offered `noctalia 5.4.0-1`; fresh CI resolved signed `5.4.1-1`. Upstream v5.4.1 assets were downloaded and SHA-256 verified; the archive contains C++ source, not a prebuilt executable. Borea keeps native packages and does not compile that source.
- Non-Arch GUI families are disabled in presentation and action handling. Omarchy handoff and the backend CLI remain available.
- Passing package/config checks do not certify real graphical login, lock/unlock, suspend or Arc A770 hardware. Installed-system verification is recorded separately.

- All six Arch/CachyOS × Hyprland/niri/MangoWC whole-drive, Secure Boot-off installs and subsequent boots pass without repair at `c37b14f`. Assertions cover OS identity, LUKS, account, network, sudo, active greeter/account services and the chosen configs. [Results](verification/noctalia-installed-2026-10-31.json). This does not certify interactive graphical login, lock/suspend, first-login app downloads or physical Arc hardware.
- Updated ISO composition and both USB boot fixtures pass; workspace tests, Rust 1.85, native compositor/Noctalia config validation and static Ratatui checks also pass. The 18-second H.264 walkthrough decodes successfully; its poster and Pages player reference the MP4.

## Account step sources — 31 October 2026

- Vesktop replaces Discord. A fresh AUR check succeeds: `vesktop-bin` 1.6.8-1 is maintained by Vendicated and packages Vencord's prebuilt release. Its pinned PKGBUILD and `.SRCINFO` were inspected; upstream checksums are declared, with no source compilation or make/check dependencies. Flathub's `dev.vencord.Vesktop` remains a fallback.
- All nine catalog AUR app names resolve to maintained recipes in the public RPC. Spotify now maps to `spotify`; Flatseal is verified in Arch extra as `flatseal` 2.4.1-1. Arch/CachyOS default app resolution has no Flatpak-only entry. [Metadata, URLs and hashes](verification/aur-app-sources-2026-10-31.json).
- Unit and plan checks cover AUR/native/Flatpak ordering, explicit source opt-out, unprivileged builds, archive mismatch refusal, failed-build checkpoints and all twelve Arch/CachyOS × Noctalia compositor × storage-layout app queues. The separate native AUR integration installs Vesktop in a disposable Arch container and replays its checkpoint; consult the PR's CI result for the executed run. It does not certify interactive login or physical hardware.
- The region lookup's endpoint, `https://geoip.fedoraproject.org/city` (already `borea_net::GEO_ENDPOINT`), returns `country_code` and `time_zone`; `borea_locale::guess` maps them through the 247 validated country defaults.

## Noctalia on Tumbleweed — 1 November 2026

- Read in CI from a fresh `opensuse/tumbleweed` container: `noctalia` 5.4.0-1.1 (binary reports v5.4.1), `noctalia-greeter` 1.7.0-1.1, `greetd` 0.10.3, `hyprland` 0.56.2-1.2, `xdg-desktop-portal-hyprland` 1.4.1, `niri` 26.04-1.5, `xdg-desktop-portal-gnome` 50.0, `xwayland-satellite` 0.8.3, `playerctl` 2.4.1, `xdg-desktop-portal-wlr` 0.8.4, `accountsservice`, `xdg-desktop-portal-gtk`, `xdg-user-dirs`, `pipewire`/`pipewire-pulseaudio` 1.6.10, `wireplumber`, `polkit`, `dbus-1`, `dejavu-fonts`, `foot` 1.28, `plymouth`, `plymouth-dracut`, `plymouth-theme-spinner` and `flatpak`, all from openSUSE-Tumbleweed-Oss.
- MangoWC is in no openSUSE repository. Of the OBS projects probed, `home:darix:playground` builds `mangowc` 0.18.4-1.1 for Tumbleweed (`/usr/bin/mango`); `home:osranze:oS` has only 0.16.2. Key: "home:darix:playground OBS Project", fingerprint `68EA EF3B AE1C 025E 3694 F11B 55B2 D116 3F56 078E`, created 2024-09-26, expiring 2026-12-05 (OBS renews project keys; Borea re-reads the digest at each review).
- The installed Noctalia Greeter provides the same `noctalia-greeter-session`, `noctalia-greeter-apply-appearance` and `tmpfiles.d/noctalia-greeter.conf` Borea configures on Arch; greetd's sysusers entry creates the `greeter` user. `start-hyprland` and `niri-session` are present.
- Borea's generated `hyprland.lua`, `niri.kdl`, `mango.conf` and Noctalia config pass Tumbleweed's own Hyprland 0.56.2 `--verify-config`, niri 26.04 `validate`, MangoWC 0.18.4 `-p` and Noctalia `config validate`, as an unprivileged user. CI repeats this ("Noctalia compositor config parsers (Tumbleweed)").
- The nightly Tumbleweed VM boot had failed at dracut's emergency shell waiting for `/dev/mapper/borea-<random>`: dracut, run chrooted, recorded the installer's own device-mapper name as root. `hostonly_cmdline="no"` now leaves root and its unlock to the kernel command line. Installed-system VM results for Tumbleweed are recorded in the PR.
