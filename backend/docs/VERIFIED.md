# Backend verification evidence

Date: 2026-10-26. This is an evidence ledger, not a completion certificate.

## Passing checks

- Workspace compilation, formatting, unit/integration tests and Clippy with warnings denied.
- Existing 31 catalog tests retained; additional core validation/navigation, locale, layout, executor redaction and service protocol tests.
- Pure job matrix: 120 combinations, four distributions × workstation/Plasma, server, minimal × whole/alongside × five hardware fixtures. Jobs serialize round-trip and retain the Review package selection; destructive and secret commands are checked.
- 512-byte and 4096-byte sector geometry; refusal of live/mounted/read-only/replaced disks and unsafe NTFS.
- Generated locale data: 327 UTF-8 locales, 590 keyboard layout/variant entries, 487 time zones, 247 country suggestions. Generation provenance is in `LOCALE-DATA.md`.
- Official public release keys, fingerprints and origin digests are recorded in `../dist/keys/PROVENANCE.md` and `fingerprints.txt`.
- Debian live QEMU guest reaches UEFI, Linux, virtio storage and DHCP. Its native debootstrap preflight downloads and validates Debian trixie's InRelease and package indexes using the Debian archive keyring.

## Checks in progress / release gates

- Debian install and subsequent real-disk boot assertions: in progress; not yet a pass.
- Actual Rust 1.85 check: toolchain installed; full check must be rerun after dependency graph changes finish.
- Native package-name validation across every catalog entry, desktop, add-on and driver branch. Comments marked `verify` remain open until live index evidence is recorded.
- Arch/Fedora/Tumbleweed signed rootfs URLs, native bootstrap/package commands and installed boot tests. In this managed environment the upstream Arch/openSUSE/Fedora mirrors and RPM Fusion/Packman endpoints returned explicit proxy CONNECT 403 denials. These cannot be certified with fabricated fixtures.
- AUR RPC and upstream AppImage release API validation; their configured hosts were also denied by the managed network policy. No unsigned source is silently accepted as signed.
- Whole/alongside × Secure Boot off/on QEMU matrix for every distro, Windows boot preservation, NVIDIA module signing/MOK and real Wi-Fi hardware activation/carry-over.
- Native package versions and transitive dependencies remain resolved by signed upstream metadata at execution; this is not a full reproducible package lockfile.

Native metadata preflight, approval checks and disk guards are enforced in code. Missing release evidence is not bypassed by calling an adapter complete. The old preview GUI is not enabled for destructive installs by this change.
