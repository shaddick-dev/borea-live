# Borea Live

Borea's own installer image. It is not a remastered distribution ISO: Buildroot compiles every part of it from upstream sources, Borea supplies its own early boot, and Limine boots it. Power it on and the installer is there.

## What happens at boot

1. **Limine** shows *Borea Live* for 3 seconds in Borea's colours, then boots. A *safe graphics* entry is there for displays that misbehave.
2. **Borea's early boot**, `borea-init` (Rust, `crates/borea-init`), runs as `/init`. It looks at every disk until it finds the one carrying this build's ID, waiting up to 30 seconds for slow USB sticks. It mounts the EROFS system image read-only straight from `live/rootfs.erofs` (no loop device), puts a writable layer in memory over it, and hands over to systemd. If anything is missing it says what, and opens busybox's rescue shell; signed builds power off instead.
3. **systemd** starts NetworkManager, then `borea-live.service`, which runs the installer full-screen in cage (a kiosk compositor) as the passwordless `borea` user. If it ever exits, it restarts.
4. **The installer** scans hardware at once, waits for NetworkManager if it is late, and rescans Wi-Fi and refreshes the connection every few seconds. If the machine is online it moves past Connect by itself. If not, the strongest network is chosen, with the cursor in its passphrase field.
5. **polkit** lets the `borea` user manage networking and disks, and switch consoles, without a password prompt, because there is no password to give. The rule covers only that user, only on the local active session, and only NetworkManager, UDisks2, and logind's console switch. polkit is built to track sessions through logind (Buildroot's default, ConsoleKit, would never see the session as local or active).

Ctrl+Alt+F2 opens a rescue console: log in as `root`, with no password. Ctrl+Alt+F1 goes back to the installer.

`borea-build` refuses to finish an image that lacks what the session needs to work: keymaps (without them cage has no keyboard), the installer's PAM session, the UEFI variables mount, and a polkit linked with logind.

## One command: build and write a stick

```sh
cargo run --release -p borea-flash -- --install            # once: adds borea-flash to ~/.local/bin
borea-flash --make                                         # from any terminal
cargo run --release -p borea-flash -- --install-launcher   # optional: a "borea-rataTUI" app launcher entry
```

The installed copy remembers this checkout, so `borea-flash --make` works from any directory. Plain `borea-flash` helps too: when this checkout has changed since the newest image, its Image panel says so, and **b** rebuilds before writing (with no image yet, it starts building straight away). It runs in any terminal, the Linux console included. The launcher entry opens a terminal running the same thing. `borea-flash --make`:

1. If the newest image is older than anything under `crates/`, `live/`, or `Cargo.lock` (or there is no image yet), it rebuilds Borea Live. The screen shows Buildroot's current package, how many packages are done, the elapsed time, and the latest output; everything also goes to `target/live/build.log`. Esc asks before stopping, and everything already built is kept. If the build fails, **r** picks up where it stopped.
2. Once built (or straight away when already up to date), it moves on to writing. The image is checked against its `.sha256`, only USB sticks are listed, and a ready one is already selected.
3. You type the stick's name (for example `sdb`) and it writes, verifies every byte, and offers to eject.

Typing the name is the one deliberate step. Everything else is automatic, and it is what keeps a launcher click from ever erasing the wrong disk. Only one build runs per checkout: a second refuses while one is running, including builds started outside the launcher.

## Building

```sh
cargo run --release -p borea-build -- iso                    # target/live/out/borea-live-<version>-<date>.iso (+ .sha256)
cargo run --release -p borea-build -- iso --configure-only   # prepare and check the configuration only
cargo run --release -p borea-build -- iso --jobs 4           # fewer parallel jobs than the default
cargo run --release -p borea-flash                           # write the newest build to a USB stick
```

No root needed. The first build downloads a prebuilt Bootlin compiler toolchain, then compiles the kernel, LLVM, Mesa, Borea's Rust, and the rest. It can take up to a few hours, depending on your machine, and about 30 GB. Later builds reuse everything unchanged, including Borea's compiled Rust dependencies (`target/live/output/borea-cargo`). `borea-build iso` (Rust, `crates/borea-build`):

1. Downloads Buildroot 2026.08.1 and checks its SHA-256.
2. Finds the host tools Buildroot needs. Any that are missing come from Nix for this build only, if Nix is installed; nothing is installed system-wide.
3. Packs this workspace with every Rust dependency vendored, so the Borea package builds offline.
4. Links `target/live/hooks/post-build` and `post-image` to itself. Buildroot runs those as its hooks, and `borea-build` acts on the name it was started as.
5. Gives the build a random ID, which the early boot uses to find its own medium.
6. Configures Buildroot, and **fails if Buildroot dropped any option**.
7. Builds several independent packages at once (Buildroot's per-package directories and top-level `make -j`), with one job per CPU but no more than one per 2 GB of memory, since LLVM's larger files take 1–2 GB each to compile, and a load-average cap so overlapping packages don't oversubscribe the machine (`--jobs N` overrides the job count; `--jobs 1` builds one thing at a time). When the toolchain or this layout changes, it starts one clean build by itself; downloads and ccache are kept. The post-image hook writes the early boot image itself (a gzip `newc` cpio of `borea-init` and the libraries it needs, read from their ELF headers), lays out the ISO, runs xorriso, and installs Limine's BIOS stage.

Try it in a VM: `qemu-system-x86_64 -enable-kvm -m 4G -cdrom target/live/out/borea-live-*.iso` (add `-bios /usr/share/edk2/x64/OVMF.4m.fd` for UEFI).

## Secure Boot (optional, off by default)

The default image is unsigned: boot it with Secure Boot turned off in firmware.

Secure Boot remains optional for the installed system as well. Choose **Leave Secure Boot off** to install without key enrollment. The installer's footer has a CPU-icon **UEFI setup** shortcut, with confirmation, to restart into firmware settings when supported. After changing the setting, boot the BOREA USB again; installer selections are not retained across that restart. Legacy BIOS target installation remains outside the current UEFI backend.

To build an optionally signed live image:

```sh
cargo run --release -p borea-build -- keys ~/borea-keys              # once: db.key (private), db.crt, db.cer
cargo run --release -p borea-build -- iso --secure-boot ~/borea-keys # borea-live-<version>-<date>-signed.iso
```

Then enroll `db.cer` in each machine's Secure Boot db, from firmware setup or with a key manager. A signed build:

- signs Limine with your key, and enrolls the BLAKE2b hash of its config, so Limine refuses a kernel or early boot image that doesn't match and keeps its config editor off;
- carries the SHA-256 of the system image in the early boot, which checks it before mounting anything;
- leaves busybox out of the early boot entirely, so there is no shell to bypass those checks (it explains the problem and powers off);
- includes `boot/limine/borea-live-db.crt` on the medium, so you can check which key signed it.

`borea-build iso` refuses a key that does not match its certificate before it starts building. `borea-build keys` refuses to overwrite an existing key.

## What is inside

| Area | Contents |
| --- | --- |
| Kernel | Latest upstream, x86-64 defaults plus `board/borea/linux.fragment`. Boot storage, USB, NVMe, ISO 9660, squashfs, and overlay are built in. GPU, Wi-Fi, and Ethernet drivers are modules. |
| Firmware | Intel, AMD, MediaTek, Realtek, Atheros, and Broadcom Wi-Fi and Bluetooth; AMD and Intel GPUs; CPU microcode |
| Graphics | Mesa with iris, crocus, i915, radeonsi, r600, nouveau, virgl, and llvmpipe (software) |
| Session | systemd with logind, cage, libinput, xkbcommon, polkit, dbus-broker, DejaVu fonts |
| Network | NetworkManager, wpa_supplicant, wireless-regdb, systemd-resolved, CA certificates |
| Disks | UDisks2, util-linux, e2fsprogs, dosfstools, gptfdisk |
| Borea | `borea-live`, `borea`, `borea-flash` in `/usr/bin`, and the `/etc/borea-live` marker |

## Layout

```text
crates/borea-build/                Rust: builds the ISO, both Buildroot hooks, the early boot image, keys
crates/borea-init/                 Rust: the early boot's /init
live/buildroot/                    a Buildroot external tree (Buildroot's own make language)
  configs/borea_live_defconfig     everything selected, nothing implied
  package/borea/                   Borea, built from this workspace
  package/limine/                  Limine 12.9.4, checksum pinned
  board/borea/
    linux.fragment                 kernel options on top of x86-64 defaults
    limine.conf                    boot menu
    rootfs-overlay/                service, user, polkit rule, NetworkManager settings
```

Everything Borea runs or builds with is Rust. The rest is configuration in the formats its tools require: Buildroot's `Config.in` and `.mk` files, the kernel options fragment, systemd units, Limine's menu, and polkit's rule, which polkit only accepts in JavaScript. Busybox is in the image only for the unsigned early boot's rescue shell.

## Not yet

- Signed releases. Secure Boot builds use your own key; an official Borea key and its custody come with release signing.
- Proprietary NVIDIA drivers. NVIDIA GPUs use nouveau, or llvmpipe.
- Reproducible build IDs. Each build gets a random ID; reproducibility comes with release signing.
