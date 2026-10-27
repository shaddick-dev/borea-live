# UI and installation handoff — 23 October 2026

This update is based on GitHub commit `5c87633` and the owner's photographs of
Borea Live. It is a source update, not a rebuilt or boot-tested ISO.

## Changed

- Wi-Fi Join shares a horizontal row with the passphrase. It appears after
  typing, stays disabled for an invalid key, and works with Enter. Open networks
  can be joined without a passphrase. Hidden networks have a separate passphrase
  so they cannot accidentally reuse a visible network's credentials.
- The rail, Connect summary and Windpad header distinguish Online, Connected
  with unconfirmed internet, local network only, captive portal, checking and
  offline. A scan failure no longer prevents connection status from refreshing.
  NetworkManager is asked to recheck connectivity after a join. A connected
  network is not automatically replaced with a suggested different network.
- Extras use plain checkbox titles, indented descriptions and separate repository
  requirements. Decorative icons were removed from the Extras section.
- Wind paths drift smoothly, with different timing and offsets each launch.
  Gust lengths vary continuously. Pause and background/reduced-motion handling
  retain the existing still rendering.

## Installation is still incomplete

The request to install every listed distribution is **not implemented by this
update**. The GUI still cannot install. Do not remove its preview indicator or
set `CAN_INSTALL` to true to work around that: those switches do not implement
storage selection, account setup, approval or execution.

| Target | Existing executable implementation | Remaining work |
| --- | --- | --- |
| Arch Minimal | CLI planner in `borea-engine::arch` and root `borea-worker`; a previous-machine VM pass is recorded in `STATUS.md` | GUI storage/account review, approved worker launch and progress; fresh image/VM verification |
| Arch desktop editions and composed desktops | Rejected by the planner's tested-edition gate | Package and service provisioning, compositor/shell configuration, per-edition install/boot/update tests |
| CachyOS | Adapter specification only | Verified repositories and keyring, CPU-specific packages, desktop/Handheld provisioning and tests |
| Omarchy | Adapter specification only | Pinned upstream integration, required storage/encryption behavior, installation and update tests |
| Fedora | Adapter specification only | Verified release/toolchain, DNF deployment, SELinux labels, boot/update tests |
| Debian | Adapter specification only | Verified release/toolchain, debootstrap/APT deployment, security updates, boot/update tests |
| openSUSE Tumbleweed | Adapter specification only | Pinned snapshot, Zypper deployment, native filesystem/update behavior, boot/update tests |
| Bazzite | Adapter specification only | Verified OCI deployment with native update/rollback behavior and tested boot integration |
| Nobara and PikaOS | Adapter specifications only | Verified upstream image/release acquisition, edition/variant provisioning, native configuration, boot/update tests |

Each adapter must resolve and verify its own upstream artifacts, create an exact
reviewable plan, preserve the distribution's native update behavior, and install
only after explicit disk approval. Extras that add repositories, Flathub, or
services need their own provisioning steps; a package name in the catalog does
not implement those steps. Secure Boot execution is also not implemented for
target installations.

The existing Arch CLI workflow is documented in `README.md` and `tests/vm/`.
It erases the explicitly selected target disk. Development tests must use
disposable virtual disks, never physical host disks.

## Validation

Relevant commands:

```sh
cargo test --locked -p borea-gui -p borea-network -p borea-engine -p borea-worker
cargo clippy --locked -p borea-gui -p borea-network --all-targets -- -D warnings
cargo fmt --all -- --check
```

New regressions cover Join visibility and position, valid submission, connection
status after a scan failure, separate connection/internet states, and bounded
continuous wind variation.

Results in this cloud environment, using Rust 1.99.1:

- The targeted test command passed. There were 54 successful test functions:
  52 exercised unit/interaction checks, and two optional screenshot functions
  returned without rendering because `WINDPAD_SHOTS` was initially unset.
- With `WINDPAD_SHOTS` set, the Join interaction and `renders_each_step` tests
  were run again and passed. They generated 13 screenshots: the Join row, all
  five live-installer steps in light and dark, and the Extras section in both
  themes. The Join row and both Extras screenshots were visually inspected.
- Clippy with warnings denied, formatting checks, and `git diff --check` passed.
- The planner/worker checks used fixtures and temporary files. No host disk was
  written, no physical Wi-Fi association was attempted, and no new installation
  or boot test was performed.

To generate the live-installer screenshots on a rendering-capable machine:

```sh
mkdir -p /tmp/borea-shots
WINDPAD_SHOTS=/tmp/borea-shots cargo test --locked -p borea-gui --test live
```

Hardware Wi-Fi, captive-portal behavior, a new Buildroot ISO and disposable-VM
installation/boot/update runs must still be checked on a suitable image-build
machine. The handoff ZIP contains source, fonts and tests, excluding `.git`,
toolchains, dependency caches, generated build output and ISO files.
