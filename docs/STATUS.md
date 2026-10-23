# Borea: current status (23 October 2026)

A short handoff for whoever picks this up next. The full history is in
`docs/handover.txt` (sessions 1–30). The plan for the product is in `README.md`
and the rest of `docs/`.

## What Borea is

Borea is an independent Linux installer that isn't tied to any one
distribution's ISO.

- **Borea Live:** its own live image, built from upstream sources by
  Buildroot, which boots with Limine.
- **Windpad:** the interface where you choose a distribution and edition.
- **Adapters:** one per distribution. Each installs its target from that
  target's own upstream tools.
- **Language and licence:** Rust 2024 workspace. The licence (Apache-2.0 OR
  MIT) is proposed but not yet adopted.

## Works today

| Area | State |
| --- | --- |
| Hardware and Wi-Fi | Read-only scan of CPU, GPUs, disks, firmware, displays and more. NetworkManager Wi-Fi through D-Bus. |
| Catalog | Validated data covering editions, compositors and shells, and extras. One naming convention; Minimal comes last. |
| Windpad / Borea Live GUI | egui interface in a Nerd Font look. Responsive cards, an offer to compose your own desktop, and extras as checkboxes. The live installer runs Connect → Machine → System → Desktop → Review, marked as a preview, and doesn't install. |
| Borea Live ISO | `cargo run --release -p borea-build -- iso`, then `target/live/out/borea-live-*.iso`. Boots under BIOS and UEFI and starts the installer automatically. The build checks that keymaps, polkit with logind, the efivars mount and the installer tools are present. |
| borea-flash | Terminal USB writer. `borea-flash --install` puts it in `~/.local/bin`. Press **b** to rebuild the ISO. Typing the device name is mandatory. |
| **Arch install** | `borea plan` verifies Arch's signed bootstrap archive and writes a plan with a digest. `sudo borea-worker PLAN --approve DIGEST --bootstrap FILE` installs Arch Minimal to a GPT/ext4 disk with Limine. |

**VM-verified:** `python tests/vm/arch_install.py` passed (run `target/vm/arch-7lc2tv_e`):
1. Installed in 11 steps in about 1 minute.
2. Booted through its own UEFI entry and Limine.
3. The new user logged in.
4. `pacman -Syu linux` ran a kernel package transaction.
5. It rebooted and the user logged in again.

## Not done / next

1. **Kernel-version upgrade:** prove an upgrade between two distinct kernel
   versions. That needs a fixture pinned to an older kernel; today's run only
   reinstalls the current kernel.
2. **GUI integration:** connect the Review step in Windpad and Borea Live to
   `borea_engine::arch::plan` and the worker. That needs polkit-mediated
   launch and IPC for progress; the worker is currently run directly as root.
3. **More editions:** Arch KDE Plasma, GNOME and Xfce, each gated by its own VM
   pass (`TESTED_EDITIONS` in `crates/borea-engine/src/arch.rs`).
4. **Wider support:** extras that need extra repositories (Gaming needs
   multilib), the BIOS boot path, and Secure Boot execution.
5. **Other distributions:** Fedora and Debian adapters, then the derivatives.
6. **Release work:** signed catalog and release hosting, CI, and a first boot
   on real hardware.

## Changes since dfd6ed3 (uncommitted, to push)

1. **Wi-Fi fix for Borea Live** (`live/buildroot/external.mk`): ships every
   Intel iwlwifi firmware (AX200, AX201, AX211, BE200 and others that
   Buildroot's per-generation options miss) plus Qualcomm WCN7850 (ath12k).
   Only the three newest firmware versions of each card are kept to hold the
   image size down. Without the firmware, NetworkManager had no Wi-Fi adapter
   and the installer couldn't get past Connect.
2. **Nord blue theme:** `crates/borea-gui/src/theme.rs`, `design/windpad.html`,
   `docs/product.md` and `docs/windpad.md` move from ivory/charcoal and
   blue-green to Nord snow storm / polar night with a frost-blue accent.
3. **Borea TUI Helper** (Codex): `borea-helper` binary
   (`crates/borea-flash/src/helper.rs`, `src/install.rs`, `README.md`) builds
   the ISO when it's missing or stale, then flashes. The ISO freshness check
   now includes the root `Cargo.toml`.

Verified: `cargo test -p borea-flash -p borea-gui` passes. The ISO
`borea-live-0.1.0-20261023.iso` was rebuilt with the firmware and theme and
boot-checked under OVMF (Machine step renders in Nord dark). Wi-Fi on real
Intel hardware hasn't been tested yet.

Suggested commit: "Ship Intel and WCN7850 Wi-Fi firmware, Nord theme, Borea
TUI Helper".

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
- If Buildroot package options change, run `make <pkg>-dirclean` once.
  Buildroot doesn't rebuild a package when its options change.
- `target/` (about 70 GB) holds Buildroot's build cache. Keep it: it's what
  makes rebuilds incremental.
- Uncommitted at handoff, ready to commit and push (see "Changes since
  dfd6ed3" below). The repository has no remote yet: add the GitHub one
  before pushing.
- Codex's coordination notes are in `docs/CODEX_CLAUDE_COORDINATION.md`.
