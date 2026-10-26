# Backend verification evidence

Date: 2026-10-26. This is an evidence ledger, not a completion certificate.

## Passing checks

- Workspace compilation, formatting, unit/integration tests and Clippy with warnings denied.
- All 69 workspace unit/integration tests pass, including the original catalog rules and additional core validation/navigation, locale, layout, executor redaction, mount cleanup, static-helper and service protocol tests.
- Pure job matrix: 120 combinations, four distributions × workstation/Plasma, server, minimal × whole/alongside × five hardware fixtures. Jobs serialize round-trip and retain the Review package selection; destructive and secret commands are checked. The 240 checked-in Review/script snapshots make changes reviewable. Rendered scripts exit before execution; only the approved service executes jobs.
- 512-byte and 4096-byte sector geometry; refusal of live/mounted/read-only/replaced disks and unsafe NTFS.
- Generated locale data: 327 UTF-8 locales, 590 keyboard layout/variant entries, 485 time zones, 247 country suggestions. Every country suggestion validates. Host-specific `localtime` and `posixrules` are excluded. Generation provenance is in `LOCALE-DATA.md`.
- Official public release keys, fingerprints and origin digests are recorded in `../dist/keys/PROVENANCE.md` and `fingerprints.txt`.
- Debian native metadata audit: all 113 native names in the full catalog/desktop/add-on/driver/helper inventory resolve from signed trixie, trixie-updates and trixie-security indexes (main, contrib, non-free, non-free-firmware). Telegram uses Flathub because `telegram-desktop` is absent. Debian COSMIC is explicitly unavailable under the native-source policy: `cosmic-session` and `cosmic-greeter` are absent. A community installer found during research converts Fedora COPR RPMs with `alien`; it was not executed or treated as native Debian support.
- Debian live QEMU guest reaches UEFI, Linux, virtio storage and DHCP. Native preflight validates trixie's signed metadata before partitioning. A disposable run successfully partitioned its qcow2, created LUKS2/btrfs, bootstrapped minbase, and installed the selected native packages and security updates. It exposed a configuration-symlink refusal and recursive-bind cleanup bug; these are fixed and the complete install/boot run must pass before certification.
- Native Debian package inspection confirms the signed shim/MOK-manager paths and cryptroot's `initramfs` crypttab option, needed when the live mapping name differs from the installed mapping. Upstream `sdbootutil` and `mokutil` source inspection informed their native CLI and secret-stdin handling.

## Checks in progress / release gates

- Debian install and subsequent real-disk boot assertions: in progress; not yet a pass.
- Rust 1.85 native workspace check and static musl target-helper build: rerunning after the latest dependency graph changes.
- Native package-name validation across Arch, Fedora and Tumbleweed. The nightly/manual `backend-packages.yml` workflow runs the same Rust inventory against each official distro container, including its standard community repos. External entries printed as `EXTERNAL` are not certified by the native audit.
- Arch/Fedora/Tumbleweed signed rootfs URLs, native bootstrap/package commands and installed boot tests. In this managed environment the upstream Arch/openSUSE/Fedora mirrors and RPM Fusion/Packman endpoints returned explicit proxy CONNECT 403 denials. These cannot be certified with fabricated fixtures.
- AUR RPC and upstream AppImage release API validation; their configured hosts were also denied by the managed network policy. No unsigned source is silently accepted as signed.
- Whole/alongside × Secure Boot off/on QEMU matrix for every distro, Windows boot preservation, NVIDIA module signing/MOK and real Wi-Fi hardware activation/carry-over.
- Native package versions and transitive dependencies remain resolved by signed upstream metadata at execution; this is not a full reproducible package lockfile.

Native metadata preflight, approval checks and disk guards are enforced in code. Missing release evidence is not bypassed by calling an adapter complete. The old preview GUI is not enabled for destructive installs by this change.
