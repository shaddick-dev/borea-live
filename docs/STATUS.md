# Borea: current status (28 October 2026)

What is true in the repository today. Earlier session handovers are in
[`docs/archive/`](archive/); the backend's design decisions are in
[`backend/docs/DECISIONS.md`](../backend/docs/DECISIONS.md).

## Layout

One Cargo workspace (`Cargo.toml`, one `Cargo.lock`), every crate under `crates/`:

- **Installer backend** (MSRV 1.85, GPL-3.0-or-later): `borea-core`,
  `borea-catalog`, `borea-backend`, `borea-job`, `borea-exec`, `borea-disk`,
  `borea-net`, `borea-http`, `borea-locale`, `borea-platform`, plus
  `borea-installd` (the root service), `borea-cli` (its command line) and
  `borea-protocol` (the socket contract both clients share). Docs, service
  files, signing keys and the disposable-VM harness are in `backend/`.
- **Live front end** (workspace Rust 1.95 baseline, Iced 0.14): `borea-gui`
  (`borea-live`, `windpad`), `borea-bootstrap` (`borea`), `borea-flash`,
  `borea-build`, `borea-init`, `borea-hardware`, `borea-host`, `borea-network`,
  `borea-distro`, `borea-live-core`, `borea-live-catalog`, `borea-testkit`.
- **Borea Live image:** `live/buildroot`, built by `borea-build iso`.

## Works

| Area | State |
| --- | --- |
| Arch/CachyOS + Hyprland, niri or MangoWC + Noctalia v5 | Native Iced storage/account/review path, Noctalia Greeter and first-login app queue. All six profiles pass whole-drive, Secure Boot-off install/boot checks. Generated configs pass all three native compositor parsers and Noctalia's validator. Signed stable v5 packages from 5.4.0 are accepted; GitHub patch-version lag no longer blocks installation. Full graphical installation and Arc hardware verification remain pending. [Details](arch-noctalia.md). |
| Installer backend | Arch rolling, Debian 13, Fedora stable, Tumbleweed. UEFI only, LUKS2 + Btrfs, whole disk or alongside Windows, optional Secure Boot. Review pins sources and returns an approval digest; install re-checks it and the disk before writing. Verified in disposable VMs (`backend/docs/VERIFIED.md`). |
| Borea Live GUI | Only some Arch installs work (including Omarchy). Other families are greyed out. Firmware-setup restart goes through `borea-installd`, which refuses while an install runs. |
| Borea Live ISO | Built with Bootlin's prebuilt glibc toolchain and per-package directories (packages build in parallel); Borea's Rust is cached between builds. Ships `borea-installd` (enabled service), `borea-cli`, the static `borea-firstboot` helper, the pinned Arch/Fedora/openSUSE keyrings, and cryptsetup, btrfs-progs, ntfsprogs, gnupg, efibootmgr, parted, openssl. |
| borea-rataTUI | The USB writer (crate `borea-flash`; `borea-helper` and `borea-flash` remain as aliases). Downloads a published ISO, then removable devices only, typed confirmation, read-back verification. Normal setup/download/write paths never fall back to compiling. Builds require explicit developer options. Published artifacts are required; no release was available when checked on this date. |
| Selected apps after boot | Backend plans queue selected Flatpak apps for noninteractive installation by the Rust first-boot service. Successful steps are checkpointed; failures retry after 60 seconds. The desktop does not wait for the downloads to finish. The Arch/CachyOS Noctalia GUI path also defers selected native apps until graphical login. Other backend profiles keep their existing native/AppImage installation steps; explicitly enabled AUR can build packages. |
| Local ISO composition | `borea-rataTUI --compose` and the failed-download screen can create an ISO without a Borea release. Docker supplies prebuilt kernel, Mesa, firmware and native tools; only Borea's Rust apps and a small Limine utility compile. Dependencies/runtime are cached. Source Buildroot builds remain explicit. See [composition](../live/compose/README.md). |
| Installer hub | Hand-off to Omarchy 4.0.6 (`borea-cli handoff omarchy --whole DISK` or `--alongside WINDOWS_PARTITION --size 60G`, `handoff.review`/`handoff.start`): downloads the pinned ISO to the stick's `BOREA_DATA` partition, checks its SHA-256 and Omarchy's signing key, shrinks Windows only for alongside, then kexecs into Omarchy's own installer via its loopback entry. In the Borea Live GUI: choosing Omarchy makes its Review step install it (pick a disk, whole or beside Windows with a size, check the plan with the service, confirm shrinking Windows, begin), tested against a fake service. **Verified in disposable UEFI VMs, both layouts** (`handoff-vm.yml`, [run](https://github.com/shaddick-dev/borea-live/actions/runs/40818351930)): Borea Live checks Omarchy 4.0.6's SHA-256 and signature, kexecs into its installer, which boots from the ISO on the stick's data partition with its configurator present; alongside, Omarchy sees the free space and Windows is exactly 40 GiB smaller, `ntfsfix`-clean, data intact; the GUI's live user reaches the service. The GUI flow itself is tested against a fake service; Omarchy's own install steps after the hand-off are Omarchy's. |
| CI | Whole workspace on stable (fmt, clippy `-D warnings`, tests); backend crates on 1.85; cargo-deny advisories and sources; one disposable Arch install and boot on every PR touching installer code; all distributions nightly. |

## Not done / next

1. **Finish a full ISO build and boot it.** The first build on Debian found two
   hidden dependencies, now fixed: Limine needs clang and ld.lld (host-clang,
   host-lld), and polkit needs systemd built first. Not yet confirmed by a
   finished build: the static glibc `borea-firstboot` link, the overlay service
   link, and per-package directories across every package. All defconfig
   options are accepted by Buildroot 2026.08.1.
2. **Wire the GUI Review step to `borea-installd`** through `borea-protocol`
   (`plan.review` → approval → `install.start` → `install.subscribe`). The GUI
   selection model (`borea-live-catalog`) must first be mapped onto the
   backend `Plan` (`borea-catalog`); that mapping is a product decision.
3. **Fedora and Debian from the ISO.** Fedora's tool root is a container image
   and needs skopeo and umoci, which Buildroot does not package. Debian needs
   debootstrap and debian-archive-keyring. Without them those installs stop
   before any disk write.
4. **Port `tests/vm/arch_install.py`** (ISO-level end-to-end) from the retired
   `borea-worker` to `borea-cli` with LUKS unlock at first boot.
5. **Licence.** `LICENSE` is Apache-2.0, the backend crates declare
   GPL-3.0-or-later, and `borea.mk` says "Apache-2.0 OR MIT (proposed)". Pick one.

## Rules the owner set

- Rust wherever possible.
- Secure Boot is opt-in, never the default.
- Never write to real disks in tests; use disposable VMs under `target/vm`.
- Typing the device name before any USB write stays mandatory.
- Commit or push only when asked.

## Working notes

- `/tmp` here is a 7.7 GB tmpfs. Put VM disks under `target/vm`.
- Under OVMF, direct kernel boot doesn't detect QEMU's emulated CD drive.
  Attach the ISO as a read-only virtio disk instead.
- Buildroot doesn't rebuild a package when its options change; `borea-build`
  does it for you, comparing `.config.old` with `.config` after configuring.
- `target/` holds Buildroot's build cache. Keep it: it's what makes rebuilds
  incremental.
- Enable units in the overlay with links, never copies: systemd ignores
  anything in a `.wants/` directory that isn't a link. The default target comes
  from `BR2_PACKAGE_SYSTEMD_DEFAULT_TARGET`, not an overlay file. borea-build's
  post-build check fails the build otherwise. (The first ISO booted to a blank
  console because of both.)
