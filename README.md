# Borea

**Go wherever the wind takes you.**

Borea is an independent, distro-agnostic online Linux installer. It is not tied to any distribution's ISO. It runs from Borea Live or any compatible live session. Windpad, one interface for every target, guides the installation. Distribution adapters are intended to download and install upstream systems, customize selected packages, and preserve native updates and boot requirements.

Windpad, a beautifully minimal landing pad, scans your hardware, connects you to Wi-Fi, and helps you choose your next Linux system. In Borea Live, selecting a family opens a separate distribution and edition page, with no choice preselected. Borea resolves the latest release and prepares the selected system's drivers, firmware, and boot integration.

## Project status

The priority is one independent Borea Live ISO that installs upstream systems
from the Arch, Fedora, Debian and openSUSE families. Each distribution retains
its own release and update model. The earlier four-base CLI profiles include
Arch rolling, Fedora Rawhide, Debian Sid and openSUSE Tumbleweed; Rawhide and
Sid are development channels, not channels to impose on their derivatives.
Borea's orchestration, planning and account logic are Rust; native distribution package managers and Linux filesystem tools remain
external components, not rewritten implementations.

The Arch Minimal CLI installer exists. The GUI is still a preview, and Fedora,
Debian and openSUSE execution adapters are not implemented. Shared Rust disk and
account preflight, account command generation, target profiles and an ordered
installation workflow are now being extracted from the Arch path. Inspect the
profiles with `cargo run --bin borea -- targets`. See
[the independent online installer work](docs/ONLINE-INSTALLER.md) for exact scope,
reference findings and the implementation/verification sequence.

Read [the vision](docs/vision.md) first, then [the handover](docs/handover.txt) for all work completed so far, the complete requirements, current limitations, and the implementation steps that remain.

## The product

| Component | Responsibility | Proposed technology |
| --- | --- | --- |
| `borea` | Terminal bootstrapper: inspect compatibility, download, verify, launch | Rust, Ratatui, Crossterm, Tokio, Reqwest |
| Windpad (`borea-gui`) | The same landing pad for every target: hardware, Wi-Fi, distribution selection, storage review, progress, recovery | Rust, egui/eframe |
| `borea-host` | Classify the installation environment by capabilities, never by distro identity | Rust, procfs/sysfs |
| `borea-engine` | Discovery, typed installation plans, validation, progress events | Rust library |
| `borea-worker` | Narrow, privileged execution of an approved installation plan | Rust executable, local IPC |
| Distribution adapters | Bootstrap, package sets, target configuration, initramfs and update integration | Reviewed Rust modules and versioned data |
| Hardware and Wi-Fi | Automatic local hardware scan, driver/firmware recommendations, integrated connectivity | Rust, sysfs/udev, NetworkManager D-Bus via zbus |
| Release catalog | Latest-release resolution, edition dropdowns, compatibility constraints | Signed metadata and upstream-specific resolvers |
| Limine integration | Install the bootloader and maintain bootable kernel entries | Pinned Limine release, adapter-specific package hooks |
| Borea Live | Reference installation environment: known drivers, firmware, Wi-Fi, compositor; built early | Minimal neutral live image; Limine as its bootloader where supported |

```text
Borea Live  or  any compatible live session
             |        (judged by capabilities)
         borea TUI
   inspect -> download -> verify
             |
          Windpad
   choose -> configure -> review
             |
   privileged worker + adapter
   fetch upstream -> install natively -> validate
             |
     Limine -> chosen Linux
```

The bootstrapper downloads a prebuilt, versioned release. End users do not need Rust, Git, or a local compiler. Source builds remain a separate contributor workflow.

## Requested product scope

| Family page | Distribution page | Edition coverage |
| --- | --- | --- |
| Arch | Arch Linux, CachyOS, Omarchy | Arch profiles; CachyOS KDE Plasma / KDE Plasma · Handheld / Minimal; upstream Omarchy experience |
| Fedora | Fedora Linux, Bazzite, Nobara | Fedora editions; all official Bazzite variants; all current Nobara editions |
| Debian | Debian, The Butterbian Project, PikaOS | Debian profiles; Butterbian Xfce / mangobutter (upstream unverified); PikaOS desktops and Standard / NVIDIA variants |
| openSUSE | openSUSE Tumbleweed | Desktop choice plus opt-in Gaming, Media codecs, Flatpak and Printing |

Family, distribution, edition and required image variants must be chosen explicitly. The catalog lists Arch rolling and Fedora/Debian stable; the CLI also has development-channel profiles described above. Derivatives follow their own current official channels, and Tumbleweed uses its latest coherent published snapshot. Resolve versions when planning and pin exact identities for the approved installation. Never silently substitute an older certified release.

Automatically scan CPU, GPUs, memory, disks, network adapters, firmware mode, displays, and device type. Keep recommendations explainable and editable. Wi-Fi is available inside both TUI and GUI. Firmware preinstallation means OS-loaded device firmware and CPU microcode; flashing device firmware is a separate explicit task.

## Implementation sequence

The first technical vertical slice remains small:

- x86-64, UEFI, Secure Boot disabled.
- Borea Live and at least one live session of a different family as test hosts. An Arch live session is a convenient first host, not a requirement.
- Arch Linux as the initial target adapter, with its tools taken from the signed upstream bootstrap archive rather than from the host.
- One selected blank or explicitly erasable disk, GPT, FAT32 EFI System Partition, ext4 root, no encryption or dual boot in the first release.
- Minimal, GNOME, KDE Plasma, and Xfce installation profiles, gated on individual test coverage.
- Automatic hardware discovery and integrated Wi-Fi; locale, keyboard, time zone, hostname, and normal user setup.
- Online package installation and Limine boot integration.
- Explicit destructive-change review, meaningful progress, redacted logs, and tested boot verification.

Fedora and Debian are priority base adapters after that slice, followed by the requested derivatives and Tumbleweed with optional extras. Every named target is part of the requested product scope. Each adapter preserves its native deployment, security, layout, and update behavior. Limine remains preferred; incompatible targets stay integration-pending until their boot path is tested, with no silent bootloader replacement.

Running from an already-installed system, installing over the running root filesystem, Windows/macOS bootstrapping, and installing directly from firmware are outside the initial slice.

## Read the specification

0. [Vision: an independent installer](docs/vision.md) — host, Windpad, and adapters as independent layers
1. [Product and user experience](docs/product.md)
2. [Rust architecture and trust boundaries](docs/architecture.md)
3. [Storage, installation, and Limine lifecycle](docs/installation.md)
4. [Distribution adapters and support policy](docs/distributions.md)
5. [Bootstrapper and release delivery](docs/delivery.md)
6. [Implementation roadmap and acceptance tests](docs/roadmap.md)
7. [Automatic hardware, Wi-Fi, firmware, and gaming](docs/hardware-network.md)
8. [Windpad and wind animation design](docs/windpad.md)
   - [Compose your desktop: compositors and shells](docs/desktops.md)
9. [Interactive interface concept](design/windpad.html) — sample hardware and simulated networking

## Repository layout

```text
README.md              this overview
Cargo.toml             the Rust workspace
crates/
  borea-gui/           apps: windpad (landing pad) and borea-live (the live installer)
  borea-bootstrap/     app:  borea (doctor, catalog, shells, wifi)
  borea-flash/         app:  borea-flash (build Borea Live, write it to a USB stick)
  borea-build/         image build: drives Buildroot, makes the ISO and signing keys
  borea-init/          image build: Borea Live's early boot /init
  borea-core/          libraries shared by the apps ...
  borea-catalog/         ... distributions, editions, shells, extras (data/*.toml)
  borea-host/            ... what the live environment can do
  borea-hardware/        ... hardware scan
  borea-network/         ... Wi-Fi through NetworkManager
  borea-distro/          ... adapter contract and readiness
  borea-testkit/       test fixtures
live/                  Borea Live: Buildroot tree and how to build it (live/README.md)
docs/                  specification, design notes, and the handover log (handover.txt)
design/windpad.html    interactive HTML concept
target/                build output, gitignored; finished ISOs in target/live/out/
```

## Building

```sh
cargo test                                          # unit and fixture tests
cargo run --bin borea -- doctor                     # read-only: environment, hardware, per-target readiness
cargo run --bin borea -- catalog                    # Windpad families, distributions, editions
cargo run --bin borea -- shells                     # compose a desktop: compositor, then shell
cargo run --bin borea -- wifi                       # connection status and networks in range
cargo run -p borea-gui --bin windpad                # Windpad, the landing pad
cargo run -p borea-gui --bin borea-live -- --window # the Borea Live installer, windowed
cargo run -p borea-flash -- --list IMAGE.iso        # USB sticks it could write, read-only
cargo run -p borea-flash -- IMAGE.iso               # write Borea Live to a USB stick
```

### Installing Arch Linux (first vertical slice, disposable disks only)

On Borea Live, as root, with UEFI and Secure Boot off:

```sh
borea plan --distro arch --channel rolling --edition minimal --extras none \
  --disk vda --user NAME --hostname HOST --timezone Area/City --locale LOCALE --keymap KEYMAP -o plan.json
borea-worker plan.json --approve DIGEST --bootstrap ~/.cache/borea/archlinux-bootstrap-….tar.zst
```

`borea plan` changes nothing: it checks the disk is free, downloads Arch's bootstrap archive, verifies its OpenPGP signature against the pinned Arch release key, and writes a plan with its SHA-256 digest. `borea-worker` runs only that exact plan (digest match), re-checks the disk, and asks for the user's password itself. Only the Minimal edition is installable, because only it has been installed and booted end to end. `tests/vm/` runs the whole thing in a throwaway QEMU disk.

To install **Borea TUI Helper** as a terminal command and app launcher:

```sh
cargo run --release -p borea-flash -- --install     # once: commands + launcher
borea-helper             # prepare ISO, choose USB, raw write, sync and verify
borea-helper IMAGE.iso   # use an existing ISO without rebuilding
borea-helper --list      # show USB candidates without writing or building
```

Open **Borea TUI Helper** from your app launcher or run `borea-helper`. It builds the ISO if missing or out of date, then asks which USB drive to erase. Type that device's name to start the raw image copy (the equivalent of `dd`), followed by flushing and byte-for-byte verification. The first ISO build compiles upstream sources and can take hours; later builds reuse previous output. `borea-flash` remains available for writing images directly.

It works in any terminal, including the Linux console without a desktop (`TERM=linux`), where it switches to 16 colours and plain marks. Run `--install` again after updating the checkout. See [the helper guide](crates/borea-flash/README.md).

The USB helper writes only to a USB stick whose name you type to confirm. `borea wifi join` adds a NetworkManager connection to the live session, asking for the passphrase without echo.

| Crate | State |
| --- | --- |
| `borea-core` | Shared facts (`Known`/`Unknown`), families, channels, support states |
| `borea-catalog` | Bundled, validated research catalog, plus compositors and shells for desktop-free editions; refuses `supported` while unsigned |
| `borea-host` | Live-environment classification and capability detection |
| `borea-hardware` | CPU and x86-64 level, microcode, memory, firmware, Secure Boot, device class, GPUs and hybrid graphics, displays, disks with partitions and in-use detection (root through LUKS/LVM, boot media), network, Bluetooth, touch and gamepad input, battery |
| `borea-network` | NetworkManager over D-Bus: connectivity and captive portals, rfkill, scanning, WPA2/WPA3/open and hidden networks, redacted secrets |
| `borea-distro` | Adapter contract, per-target acquisition strategy, readiness assessment |
| `borea-bootstrap` | `borea doctor`, `catalog`, `shells`, `wifi`, `plan`; download/verify/launch arrives in M1 |
| `borea-engine` | Installation planning: request → checked, pinned `InstallPlan` with a digest; Arch adapter (signed bootstrap, packages, layout) |
| `borea-worker` | Root-only executor of one approved plan, in a private mount namespace: partition, format, pacstrap, configure, Limine, validate |
| `borea-gui` | **`borea-live`**, the guided full-screen installer Borea Live boots into (Connect → Machine → Family → Distribution → Desktop → Review, with a step rail), and **`windpad`**, the landing pad. Both share the selection model, theme, cards, wind, and Wi-Fi. Windpad itself: live hardware strip, family cards, compose dialog, preview under each card, hardware and Wi-Fi drawers, light and dark themes, wind animation that respects reduced motion |
| `borea-flash` | TUI that writes the Borea Live ISO to a USB stick: removable/USB disks only, refuses the system disk and the image's own disk, checks the published SHA-256, requires typing the device name, opens it exclusively (directly as root, otherwise through UDisks2/polkit), and verifies every byte read back from the device |
| `live/` | Borea Live, the standalone installer image: built entirely from upstream sources with Buildroot, with Limine as bootloader; boots straight into `borea-live`. Built without root by `borea-build` |
| `borea-build` | Builds Borea Live: drives Buildroot, serves as its post-build and post-image hooks, writes the early boot image, signs for Secure Boot, makes signing keys, refreshes the HTML concept's catalog data |
| `borea-init` | Borea Live's early boot `/init`: finds this build's medium, verifies it when signed, mounts the EROFS system with a RAM overlay, starts systemd |
| `borea-testkit` | `FakeRoot` fixture trees standing in for `/` in probe tests |

## Project principles

- Keep host and target independent: the environment Borea runs in never decides what you can install.
- Preserve the chosen distribution's native package manager and update path.
- Preview an exact installation plan before changing storage.
- Treat download verification, installation recovery, and the first kernel upgrade as core functionality.
- Keep the GUI unprivileged and the privileged operation surface small.
- Prefer tested support over broad compatibility claims.
- Make the simple path understandable while leaving room for advanced options later.

Recommended licensing: Apache-2.0 OR MIT for Borea-authored code, subject to the maintainer's decision. Third-party programs, distribution packages, artwork, and redistributed artifacts retain their own licenses and redistribution obligations. No project license is adopted by this proposal alone.
