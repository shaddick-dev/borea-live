# Borea backend — implementation plan for Codex

Build the whole backend of the Borea live installer in Rust, once, correctly. This document is the
contract. Where it says MUST, do exactly that; where it says *verify*, check against the live distro
package index before release and record the result in `docs/VERIFIED.md`.

The visual design (the HTML prototype on the design canvas) is the reference for *what the user sees and
decides*. This plan covers everything behind it: hardware, network, locale, catalogue, disk, the install
job, and the privileged service that runs it. The UI itself is out of scope, but the API it calls is not.

---

## 0. Hard constraints (never violate)

1. **Distros:** Arch Linux, Debian 13 (stable), Fedora (current release), openSUSE Tumbleweed, shown in that order. **No NixOS** anywhere.
2. **Online only.** No offline install path. The live ISO must reach the chosen distro's mirrors.
3. **Sources:** only the chosen distro's own repos (base + its standard community repos), the **AUR on Arch only**, **Flathub**, and an app's **upstream AppImage as a last resort**. Never another distro's repos, never vendor repos — with one documented exception: openSUSE's NVIDIA repo, configured through openSUSE's own `openSUSE-repos-NVIDIA` package, only when an NVIDIA GPU is present.
4. **Nothing touches the disk before the user presses Install on Review (step 08).** Everything before that is a pure, serialisable `Plan`.
5. **Drivers are never a question.** Hardware is probed at boot; drivers and the sources they need are chosen automatically.
6. **Every language, locale, keyboard layout/variant and time zone** the system data knows must be selectable. Wi-Fi comes first so IP-based defaults can be suggested.
7. **Secrets** (passphrases, Wi-Fi keys) are never logged, never on a command line, never written to disk in plain text except where the target OS requires it (NetworkManager keyfile, mode 0600).
8. **Zero surprises:** every command the installer will run can be rendered as a readable script before running (dry run).

---

## 1. What already exists — keep and build on it

`borea-catalog` (this crate, zero dependencies, 31 tests). It is correct and is the source of truth for:

| Module | Provides |
|---|---|
| `source.rs` | `Distro`, `Repo`/`REPOS` (incl. `debian.non-free-firmware`, on-demand `opensuse.nvidia`), `Source` (Repo/Aur/Flathub/AppImage), `Sources` (switches, `require()` locks driver sources) |
| `catalog.rs` | `APPS` (desktop apps only, app-store categories), `PRESETS` (apps + add-ons), `SEARCH_ONLY` |
| `resolve.rs` | `Prefer`, `candidates`, `resolve`, `Selection::toggle` (auto-enables needed source), `plan` (batches per source, stranded apps) |
| `groups.rs` | `GROUPS` (4 environments, add-ons in `SECTIONS`), `PKGS` (key → name per distro), `DESKTOPS`, `REQUIRED`, `Picks`, `plan_groups_with`, `FIREWALL`, `addon_for_app` |
| `hardware.rs` | `scan(root, names)` → `Hardware` (DMI, GPUs + generation, boot VGA, panel owner, MUX, Secure Boot, CPU, SOF, Broadcom wl, fingerprint), `tidy_name`, `pci_ids`, `samples()` (fixtures only) |
| `drivers.rs` | `plan_drivers(&Hardware, Distro)` → `DriverPlan` (found lines, packages by source, sources to lock, services, kernel args, modprobe, MOK, warning) |
| `probe.rs` + `bin/borea-probe` | boot-time scan → `/run/borea/hardware.json`; `dist/borea-probe.service` |

Rules: do not duplicate this data elsewhere. Extend it in place (and its tests) when the backend needs more.
Every `// verify` in it is an open item for milestone M8.

---

## 2. Workspace layout

Convert the repo to a Cargo workspace:

```
borea/
  Cargo.toml                 [workspace], shared lints, edition 2024, MSRV 1.85
  crates/
    borea-catalog/           (existing, moved here unchanged)
    borea-core/              Plan, Step, validation, Review summary — pure, no I/O
    borea-locale/            languages, locales, keyboards, time zones, IP → defaults — data + pure logic
    borea-net/               NetworkManager over D-Bus: scan, connect, status; connectivity; GeoIP
    borea-disk/              disk/partition/OS probing (read-only) and DiskPlan → layout (pure)
    borea-job/               Plan + Hardware + Disk facts → Job (ordered, typed Steps) — pure
    borea-backend/           per-distro knowledge used by borea-job (bootstrap, repos, bootloader, initramfs…)
    borea-exec/              runs a Job: commands, chroot, mounts, logs, progress, retry — THE ONLY WRITER
    borea-installd/          privileged service: JSON-RPC over a Unix socket, owns exec
    borea-cli/               dev tool: probe, render a Job, dry-run, run a plan file headless
  dist/                      systemd units, polkit rule, tmpfiles
  tests/vm/                  QEMU/OVMF integration harness
  docs/                      this plan, VERIFIED.md, PROTOCOL.md (generated), DECISIONS.md
```

Dependency policy (keep it small; anything else needs a line in `docs/DECISIONS.md`):
`serde`, `serde_json`, `toml`, `thiserror`, `tracing` + `tracing-subscriber`, `zbus` (D-Bus), `zeroize`,
`sha2`, `ureq` (blocking HTTPS, rustls), `nix` (mount/chroot/signals). Dev: `insta`, `tempfile`, `proptest`.
No async runtime in pure crates; `borea-installd` may use a small thread pool, no tokio required.

Code rules: `#![forbid(unsafe_code)]` everywhere; `clippy -D warnings`; no `unwrap`/`expect`/`panic!`
outside tests; errors are `thiserror` enums with a human sentence; every external command goes through
`CommandSpec` (argv vector, never a shell string); secrets are `Secret(Zeroizing<String>)` and only
reach commands through stdin.

---

## 3. Boot sequence on the live ISO

1. `borea-probe.service` (exists) → `/run/borea/hardware.json`.
2. `borea-installd.service` (root) starts, loads hardware.json, opens `/run/borea/installd.sock`
   (mode 0660, group `borea`).
3. NetworkManager starts; installd begins a Wi-Fi scan immediately.
4. The UI starts as user `borea` in a kiosk compositor (`cage`), talks only to the socket.
5. Kernel option `borea.motion=0` → UI starts with motion off (installd just exposes it in `system.info`).

The live ISO needs: systemd, NetworkManager, wpa_supplicant/iwd, cryptsetup, btrfs-progs, dosfstools,
e2fsprogs, ntfs-3g (ntfsresize, ntfsinfo), util-linux (sfdisk, lsblk, wipefs), gptfdisk (sgdisk), parted,
os-prober, curl, gnupg, debootstrap, zstd, xz, hwdata (pci.ids), squashfs/tar tools, plus the UI.
The live ISO's own base distro does not matter (see §8.1); pick the one with the newest kernel and
firmware (recommendation: Arch-based ISO).

---

## 4. Core types (`borea-core`)

All `Serialize + Deserialize + Clone + Debug + PartialEq`. A `Plan` saved to TOML replays headlessly.

```rust
pub struct Plan {
    pub net: NetChoice,                 // Wifi { ssid } | Ethernet | Hidden { ssid }  (credentials NOT here)
    pub locale: LocaleChoice,           // lang ("en_GB.UTF-8"), keyboard { layout, variant }, timezone ("Europe/London")
    pub distro: Distro,
    pub desktop: Option<Desktop>,       // None == "No desktop" (step 04 skipped)
    pub env: EnvId,                     // "minimal" | "server" | "server-gui" | "workstation"
    pub addons: BTreeSet<AddonId>,
    pub picks: Picks,                   // ticks/unticks inside groups (borea-catalog)
    pub apps: Selection,                // desktop apps
    pub prefer: Prefer,                 // distro packages first | Flathub first
    pub sources: SourceSwitches,        // user's switches; driver-required ones are re-locked on load
    pub disk: DiskPlan,
    pub user: Identity,
}

pub enum Desktop { Plasma, Gnome, Cosmic, Budgie }

pub enum DiskPlan {
    Alongside { disk: DiskId, windows_part: PartId, linux_size: Bytes },
    Whole { disk: DiskId },
}

pub struct Identity {
    pub full_name: String,
    pub username: String,                // validated: ^[a-z_][a-z0-9_-]{0,31}$, not reserved
    pub hostname: String,                // "computer name": RFC 1123 label, ≤ 63
    pub same_passphrase: bool,           // true: one passphrase for disk, login, sudo and root
    // passphrases are NOT part of Plan; they live in Secrets, sent separately to installd
}

pub struct Secrets { pub wifi: Option<Secret>, pub passphrase: Secret, pub admin: Option<Secret> }

pub struct DiskId { pub path: PathBuf, pub serial: Option<String>, pub size: Bytes, pub model: String }
```

`DiskId` carries serial + size so exec can refuse if `/dev/nvme0n1` is not the same disk the user saw.

### Steps and navigation

```rust
pub enum Step { Network, Locale, Distro, Desktop, System, Disk, You, Review, Install, Done, Recover }
impl Step {
    pub fn next(self, p: &Plan) -> Step;   // Distro → System when p.desktop.is_none()
    pub fn prev(self, p: &Plan) -> Step;   // System → Distro when p.desktop.is_none()
}
```

### Validation (`Plan::validate(step) -> Result<(), Hint>`)

`Hint { field: &'static str, message: String }` — one short human sentence (the UI shows it as-is).

| Step | Rules |
|---|---|
| Network | connectivity check passed (installd state) |
| Locale | lang exists in locale data; keyboard layout/variant exists in xkb data; tz exists in tzdata |
| Distro | always ok; attach `DriverPlan.warning` as a non-blocking notice |
| Desktop | desktop set unless `None` was chosen on 03 |
| System | env exists; env with desktop ⇒ `desktop.is_some()`; no stranded apps (or user accepted); every picked app resolves on this distro |
| Disk | disk exists and matches `DiskId`; not the live USB; target ≥ 25 GiB (≥ 40 GiB with a desktop); Alongside: Windows partition resizable (§9.2) and Windows keeps ≥ 30 GiB free |
| You | username/hostname valid; passphrase ≥ 8 chars, matches confirm; admin passphrase present when `!same_passphrase` |
| Review | all of the above, re-run in installd before starting (never trust the UI) |

### Review summary (`borea_core::review(&Plan, &Hardware) -> Review`)

What step 08 shows: system/machine/graphics/install/apps/disk/you/region rows, every package grouped
by source (env, add-ons, drivers, apps), totals (packages, sources, download/installed size estimate),
warnings, stranded apps, and whether a first-boot key approval (MOK) will be needed.
This is the same data the Job is compiled from — they must never disagree (test it).

---

## 5. Locale data (`borea-locale`)

Generate tables at build time (a `build.rs` or a checked-in generator under `tools/`), committed as
Rust source so the ISO needs no runtime data parsing:

- **Locales:** glibc `SUPPORTED` list (all UTF-8 locales). Display names in the locale's own language
  and in English from CLDR (`English (United Kingdom)`, never "as in Britain").
- **Keyboards:** xkeyboard-config `evdev.xml`: every layout and variant with its description, plus the
  console keymap mapping from systemd's `kbd-model-map`. Show "English (UK) · QWERTY"-style labels:
  the layout family (QWERTY/AZERTY/QWERTZ/Dvorak/Colemak…) derived from the variant/layout.
- **Time zones:** tzdata `zone1970.tab` (+ `backward` aliases accepted). Display with spaces
  (`America/New York`), store with underscores.
- **Country → defaults:** CLDR likely-subtags + territory info: country → default locale, keyboard,
  first time zone of that country.

Behaviour (matches the prototype):
- After connecting, `geo.suggest` returns country + time zone (see §6). Pre-select locale, keyboard
  and time zone from it; the user can change anything.
- Choosing a language re-derives keyboard (`en_US` → `us`, `en_GB` → `gb`) unless the user already
  touched the keyboard; same for time zone.

---

## 6. Network (`borea-net`)

NetworkManager over D-Bus (`zbus`), no `nmcli` parsing.

- `scan()` → `Vec<AccessPoint { ssid, strength, security, known }>`, rescans every 10 s while on step 01,
  deduplicated by SSID (strongest BSSID wins). Ethernet shown separately with no strength.
- `connect(Wifi { ssid }, Secret)` → creates a system connection; reports `Connecting`, `Connected`,
  `WrongPassword`, `Failed(reason)`.
- `connectivity()` → NM connectivity state, then an HTTPS HEAD to the chosen distro's mirror later.
- **GeoIP:** `GET https://geoip.fedoraproject.org/city` (Fedora's public endpoint, used by Anaconda) →
  `country_code`, `time_zone`. Timeout 3 s, one retry, failure is silent (defaults stay en_US/UTC until
  the user picks). Make the endpoint a constant in one place.
- The connection used in the live session is copied to the target (§8.9) so first boot is online.

---

## 7. Disk (`borea-disk`)

### 7.1 Probe (read-only)

`lsblk --json --bytes -o NAME,PATH,SIZE,TYPE,FSTYPE,PARTTYPE,PARTLABEL,LABEL,UUID,SERIAL,MODEL,RM,RO,MOUNTPOINTS,TRAN`
plus `blkid` for detail. Produce `Disk { id, parts, transport, removable }` and `Part { fs, size, used,
label, efi: bool, bitlocker: bool, ntfs_hibernated: bool }`.

- Exclude the live medium (the device backing `/run/archiso/bootmnt` or equivalent; also any disk
  holding the mounted ISO squashfs).
- `os-prober` (read-only) for existing systems; detect Windows by the ESP's `EFI/Microsoft/Boot/bootmgfw.efi`.
- BitLocker: NTFS boot sector OEM ID `-FVE-FS-`. Hibernated/Fast Startup: `ntfs-3g.probe --readwrite`
  exit code 14, or `ntfsinfo` hiberfile check.
- Free space of NTFS: `ntfsresize --info --force --no-progress-bar` → minimum size.

### 7.2 Layouts (pure)

UEFI only. If the machine booted in legacy BIOS mode, block at step 06 with a clear message.

**Whole drive** (GPT, wipe signatures):

| # | Size | Type | FS | Mount |
|---|---|---|---|---|
| 1 | 1 GiB | ESP | FAT32 | `/boot` (Arch, openSUSE: systemd-boot) or `/boot/efi` (Debian, Fedora: GRUB) |
| 2 | 1 GiB | XBOOTLDR / Linux | ext4 | only for GRUB distros: `/boot` (unencrypted; GRUB can't reliably unlock LUKS2 Argon2id) |
| 3 | rest | Linux root (x86-64) | LUKS2 → btrfs | `/` |

**Alongside Windows:** shrink the Windows NTFS partition to leave `linux_size`, reuse the Windows ESP
if it has ≥ 250 MiB free, otherwise create a 1 GiB XBOOTLDR (vfat) for systemd-boot distros; GRUB
distros always get their own 1 GiB ext4 `/boot`. Never move or delete Windows partitions.

Refuse (with a sentence and how to fix it in Windows) when: BitLocker on; NTFS hibernated (Fast
Startup); NTFS dirty; not enough free space; ESP missing.

**Encryption:** LUKS2, Argon2id, AES-XTS 512, label `borea_root`, passphrase = user passphrase (always
encrypted, as the prototype states). Unlocked in the initramfs (systemd `sd-encrypt` / dracut `crypt` /
Debian `cryptsetup-initramfs`). TPM2 auto-unlock is out of scope for v1.

**btrfs subvolumes**, per distro convention:
- Arch: `@`, `@home`, `@var_log`, `@cache` (`/var/cache`), `@snapshots` (`/.snapshots`)
- Fedora: `root` → `/`, `home` → `/home` (Fedora's layout)
- openSUSE: its standard snapper layout (`@`, `@/home`, `@/root`, `@/opt`, `@/srv`, `@/usr/local`, `@/var`, `@/.snapshots`) and install `snapper` *verify against Agama's layout*
- Debian: `@`, `@home`, `@var_log`
Mount options: `compress=zstd:1,noatime`.

**Swap:** zram via `zram-generator` (`zram-size = min(ram / 2, 8192)`); no swap partition; hibernation
not supported in v1.

---

## 8. The Job (`borea-job` + `borea-backend`)

`compile(plan: &Plan, hw: &Hardware, disks: &DiskFacts, mirrors: &Mirrors) -> Result<Job, CompileError>`
is pure. A `Job` is `Vec<Stage>`; a `Stage` has a user-facing title (the install screen's sentence),
a progress weight and `Vec<Action>`.

```rust
pub enum Action {
    Run(CommandSpec),                       // argv, env, cwd, chroot: Option<PathBuf>, stdin: Option<SecretRef>, timeout, retry: RetryPolicy
    WriteFile { path, contents, mode, owner },
    AppendFile { path, line },
    Mount(MountSpec), Unmount(PathBuf),
    Download { url, sha256: Option<String>, sig: Option<SigSpec>, to: PathBuf },
    Note(String),                           // for the log only
}
```

`Job::render_script()` → a readable bash script (secrets shown as `<passphrase>`). Snapshot-test it.

Stages (titles are what step 09 shows):

| # | Title | Weight |
|---|---|---|
| 1 | Making space. / Clearing the drive. | 5 |
| 2 | Locking your space. | 3 |
| 3 | Installing {Distro}. | 35 |
| 4 | Setting up the NVIDIA driver. / Setting up drivers. | 15 |
| 5 | Adding {Desktop}. / Setting up {env}. | 20 |
| 6 | Adding your apps. | 15 |
| 7 | Finishing up. | 7 |

### 8.1 Bootstrap strategy (host-agnostic)

Use each distro's **own** tooling, not reimplementations, from a verified official rootfs. Each runs
inside a throwaway "tool root" in RAM (`/run/borea/tools/<distro>`), with the target mounted at
`/mnt` inside it:

| Distro | Tool root | Bootstrap command |
|---|---|---|
| Arch | `archlinux-bootstrap-x86_64.tar.zst` + `.sig` (verify with the Arch master keys shipped on the ISO) | `pacstrap -K /mnt base …` |
| Debian | none: `debootstrap` runs on the ISO with `debian-archive-keyring` | `debootstrap --variant=minbase --include=… trixie /mnt https://deb.debian.org/debian` |
| Fedora | official Fedora Container Base image (verify CHECKSUM + Fedora GPG key) | `dnf --installroot=/mnt --releasever=N --use-host-config install @core …` *verify flags for dnf5* |
| openSUSE | official Tumbleweed container rootfs (verify .sha256 + openSUSE key) | `zypper --root /mnt --gpg-auto-import-keys install -t pattern minimal_base …` *verify* |

After bootstrap, all further package work runs **inside the target** (`arch-chroot`-style chroot with
`/proc`, `/sys`, `/dev`, `/run`, `/sys/firmware/efi/efivars` and resolv.conf bound) using the target's
own package manager. Keyrings and repo signatures stay verified; never pass `--nogpgcheck`.

### 8.2 Mirrors

- Arch: rank mirrors by country from GeoIP with the mirror status JSON (`archlinux.org/mirrors/status/json/`), 10 fastest HTTPS → `/etc/pacman.d/mirrorlist`.
- Debian: `https://deb.debian.org/debian` (CDN).
- Fedora: default metalink.
- openSUSE: default `download.opensuse.org` (MirrorCache).

### 8.3 Repos and sources (only what the Plan + DriverPlan switched on)

| Distro | Source | How |
|---|---|---|
| Arch | multilib | uncomment `[multilib]` in `/etc/pacman.conf` |
| Arch | AUR | no helper installed by default; build each AUR package in the target as a temporary unprivileged user `borea-build` with `makepkg -s --noconfirm` from `https://aur.archlinux.org/<pkg>.git`; resolve AUR deps via the AUR RPC v5; remove the user afterwards |
| Debian | contrib, non-free, non-free-firmware | deb822 `/etc/apt/sources.list.d/debian.sources` with the components switched on; security + updates suites |
| Fedora | RPM Fusion free / nonfree | install `rpmfusion-free-release` / `rpmfusion-nonfree-release` for the target's release from `download1.rpmfusion.org`, import keys |
| openSUSE | Non-OSS | enabled by default on Tumbleweed; ensure enabled |
| openSUSE | Packman | `zypper ar -cfp 90 https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Tumbleweed/ packman`, then `zypper dup --from packman --allow-vendor-change` for codec packages only *verify mirror* |
| openSUSE | NVIDIA | `zypper in openSUSE-repos-NVIDIA` *verify name* (only with an NVIDIA GPU) |
| all | Flathub | install `flatpak`; `flatpak remote-add --system --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo` |
| all | AppImage | §8.8 |

### 8.4 Packages (one batch per source, in this order)

1. Base system: `plan_groups_with(env, addons, picks, desktop, distro).packages` + desktop meta-package and display manager from `DESKTOPS`.
2. Drivers: `DriverPlan.packages` (already grouped by source).
3. Apps: `resolve::plan(apps + add-on apps)` batches: repo → AUR → Flathub → AppImage.

Use non-interactive flags: `pacman -S --needed --noconfirm`, `apt-get install -y --no-install-recommends` (Debian: install recommends only for desktop meta-packages), `dnf install -y`, `zypper --non-interactive install --auto-agree-with-licenses`.
Debian NVIDIA needs `linux-headers-amd64` first; Fedora akmods: after installing, run `akmods --force --kernels <installed kernel>` in the chroot so the module exists on first boot.

### 8.5 System configuration

- `systemd-firstboot --root=/mnt --locale=… --keymap=… --timezone=… --hostname=… --force` (works on all four).
- Locale generation: Arch/Debian `locale.gen` + `locale-gen`; Fedora `glibc-langpack-<lang>`; openSUSE `glibc-locale`.
- X11/Wayland keyboard: `/etc/X11/xorg.conf.d/00-keyboard.conf` (layout, variant) — read by GNOME/Plasma/COSMIC/Budgie defaults.
- fstab and crypttab from UUIDs (`genfstab`-equivalent written by Borea, not by a tool).
- zram-generator config; `fstrim.timer`.
- Firewall (when `firewalld` is part of the env): `firewall-offline-cmd --add-service=…` / `--add-port=…` from `GroupPlan.firewall`.
- Services: `systemctl --root=/mnt enable …` for NetworkManager, display manager, firewalld, chronyd/sshd/cockpit.socket when installed, `DriverPlan.services`, `switcheroo-control`, power daemon.
- Default target: `graphical.target` with a desktop, else `multi-user.target`.
- `modprobe.d/borea.conf` from `DriverPlan.modprobe`.

### 8.6 Users

- Create the user with full name; groups: `wheel` (Arch/Fedora/openSUSE) or `sudo` (Debian), plus `video`, `audio` where the distro still uses them.
- Passwords via `chpasswd` with stdin, inside the chroot.
- `same_passphrase = true`: user password = LUKS passphrase = root password.
- `same_passphrase = false`: user password = LUKS passphrase; root password = admin passphrase; sudoers drop-in `Defaults rootpw` so admin actions ask for the admin passphrase. *Confirm this reading with the designer before M5 is closed.*
- Never leave root with an empty password; never enable root SSH login.

### 8.7 Bootloader and initramfs

| Distro | Bootloader | Secure Boot | Initramfs |
|---|---|---|---|
| Arch | systemd-boot (`bootctl install`), UKI optional later | if SB is on: warn on 03 (exists); in Setup Mode use `sbctl create-keys && sbctl enroll-keys -m` and sign; otherwise instruct to disable SB | mkinitcpio with `systemd`, `sd-vconsole`, `sd-encrypt` hooks |
| Debian | GRUB (`grub-efi-amd64-signed`, `shim-signed`) | via shim | initramfs-tools + `cryptsetup-initramfs` |
| Fedora | GRUB2 + shim (BLS entries) | via shim | dracut |
| openSUSE | systemd-boot via `sdbootutil` (Tumbleweed's default for new installs since 2026) *verify package set* | shim-signed systemd-boot *verify* | dracut |

Kernel command line: `rd.luks.name=<uuid>=root root=/dev/mapper/root rootflags=subvol=<root subvol>` (dracut/systemd style; Debian via crypttab) + `DriverPlan.kernel_args`. Write it where each bootloader expects it (`/etc/kernel/cmdline`, `/etc/default/grub`).
Dual boot: systemd-boot finds Windows Boot Manager on the same ESP automatically; GRUB distros set `GRUB_DISABLE_OS_PROBER=false` and run os-prober.

### 8.8 AppImages (last resort only)

GitHub API `releases/latest` for `App.appimage` → pick the `x86_64` `.AppImage` asset; verify against
a published SHA-256 if the release has one, otherwise record "unverified" in the log and on Review.
Install to `/opt/borea/appimages/<id>/`, extract icon and `.desktop` with `--appimage-extract` (no FUSE
needed), rewrite `Exec=`, install into `/usr/local/share/applications`. Add a weekly
`borea-appimage-update.timer` that re-checks releases.

### 8.9 Network carry-over

Copy the live session's NetworkManager keyfile for the connected network into
`/mnt/etc/NetworkManager/system-connections/` (0600, root) so the installed system is online at first
login (Flatpak and AUR fallbacks below depend on it).

### 8.10 First boot (`borea-firstboot.service`, oneshot, removes itself)

Runs only what could not be done in the chroot: Flatpak installs that failed there, AppImage updates,
`fwupd` metadata refresh. It never asks questions. The Done page text covers what the person sees:
"Restart, then remove the USB." and, when `DriverPlan.mok`, the one-time "Enroll MOK" instruction —
the MOK password set during install is the user passphrase (`mokutil --import` with it on stdin).

---

## 9. Execution (`borea-exec`) — the only writer

- Runs a `Job` stage by stage; each `Action` gets a stable id `stage.index`.
- Streams `Event::{StageStarted, Progress(f32), Line(Stream, String), ActionDone, Failed(Fault), Finished}`.
  Progress = weighted stage completion, smoothed; package managers' own progress parsed where cheap
  (pacman/dnf/zypper/apt percentage lines), otherwise per-package counting.
- Logs everything (secrets redacted) to `/run/borea/install.log` and, once the target is mounted,
  `/mnt/var/log/borea/install.log`.
- `Fault { action_id, title, command (redacted), exit_code, stderr_tail (last 40 lines), suggestion }`.
  Suggestions from a small table: mirror/HTTP errors → "Check the connection and retry"; GPG/key
  errors → refresh keyring then retry; disk full; package not found → report as a catalogue bug.
- Retry: actions marked idempotent can be retried from Recover; partitioning is never retried
  automatically after it has started writing.
- Abort before stage 1 is free; after stage 1 the UI must not offer abort (matches the prototype:
  no back during install).
- Before stage 1: re-read the disk, compare `DiskId` (path + serial + size), refuse on mismatch; check
  AC power on laptops (warn if on battery < 30 %).
- On success: unmount cleanly, close LUKS, `sync`.

---

## 10. `borea-installd` and the UI protocol

JSON-RPC 2.0, newline-delimited, over `/run/borea/installd.sock`. The UI never runs as root. Document
every method in `docs/PROTOCOL.md` (generate from the Rust types).

| Method | Returns |
|---|---|
| `system.info` | ISO version, motion default, UEFI/BIOS, Secure Boot |
| `hardware.get` | the `/run/borea/hardware.json` report |
| `net.scan` / `net.connect {ssid, secret}` / `net.status` | access points, connection state |
| `geo.suggest` | `{ country, timezone, locale, keyboard }` |
| `locale.languages` / `locale.keyboards` / `locale.timezones` | full lists with display names |
| `catalog.get {distro}` | repos, apps, presets, groups, desktops, firewall (same JSON as `export_json`) |
| `disks.list` | disks, partitions, Windows/BitLocker/hibernation flags, max Linux size |
| `plan.validate {plan, step}` | `null` or `Hint` |
| `plan.review {plan}` | the Review summary |
| `plan.render {plan}` | the dry-run script |
| `install.start {plan, secrets}` | job id (re-validates everything) |
| `install.events` | subscription: Event stream |
| `install.retry {action_id}` / `install.log` | |
| `system.reboot` | |

Secrets travel only in `net.connect` and `install.start`, are held in `Zeroizing` memory, and are
dropped when the job ends.

---

## 11. Testing (required, not optional)

1. **Unit + property tests** in every pure crate (validation, navigation incl. No desktop, layouts,
   size maths, locale defaults, resolution).
2. **Snapshot tests** (`insta`) of `Job::render_script()` for a matrix:
   4 distros × {workstation+plasma, server, minimal+no desktop} × {whole, alongside} ×
   hardware samples {tuf-f15, gtx1080, rtx5070, rx7800, thinkpad}. Review summary snapshots too.
3. **Consistency tests:** Review summary package list == Job package list for every matrix entry.
4. **Package-name verification** (CI, nightly): for each distro, in its official container, check every
   name Borea can emit exists (`pacman -Si`, `apt-cache show`, `dnf repoquery`, `zypper info`),
   including `PKGS`, `DESKTOPS`, `APPS.native`, driver packages. Flathub ids via the Flathub API, AUR via
   RPC. Failures block release. Results → `docs/VERIFIED.md`.
5. **VM integration** (`tests/vm/`, nightly): QEMU + OVMF, virtio disk, boots the ISO with a
   `borea.autoplan=<toml>` kernel option, installs headlessly through `borea-cli`, reboots, and asserts:
   the system boots to the right target, LUKS unlocks with the passphrase, user can sudo, network is up,
   picked packages and Flatpaks are present, firewall rules exist. Matrix: 4 distros × whole/alongside
   (an NTFS fixture image) × SB off/on (OVMF secboot vars). GPU drivers can't be tested in VMs —
   covered by snapshots and the hardware fixtures.
6. **Hardware fixtures:** add a sysfs fixture tree (like `tests/drivers.rs`) for every new probe rule.

---

## 12. Milestones (do them in order; each ends green in CI)

| M | Deliverable | Done when |
|---|---|---|
| M0 | Workspace, lints, CI (fmt, clippy, test), move `borea-catalog` | existing 31 tests pass in the workspace |
| M1 | `borea-core`: Plan, Identity, DiskPlan, Step nav, validation, Review summary, TOML round-trip | unit tests for every rule in §4; No-desktop navigation tested |
| M2 | `borea-locale` generated tables + defaults | every glibc UTF-8 locale, every xkb layout/variant, every zone1970 zone present; en_US→us, en_GB→gb, fr_FR→fr/AZERTY, de_DE→de/QWERTZ tests |
| M3 | `borea-net` (NM D-Bus, GeoIP) + `borea-cli net` | manual run on hardware; unit tests with a mock bus trait |
| M4 | `borea-disk` probe + layouts | fixtures for: empty disk, Windows + BitLocker, Windows hibernated, small ESP, live USB exclusion |
| M5 | `borea-backend` + `borea-job` for all four distros; `plan.render` | snapshot matrix (§11.2) reviewed and committed; consistency test passes |
| M6 | `borea-exec` with events, logs, redaction, retry, DiskId guard | dry-run executor test; redaction test (no secret ever in logs) |
| M7 | `borea-installd` + protocol doc + `borea-cli install --plan x.toml` | VM matrix installs and boots (whole disk, SB off) for all four distros |
| M8 | Alongside Windows, Secure Boot paths, AppImage, AUR builder, first-boot service, package verification | full VM matrix green; `docs/VERIFIED.md` has no open `verify` items |

---

## 13. Open decisions (record answers in `docs/DECISIONS.md`)

1. Live ISO base distro (recommended: Arch, for the newest kernel and firmware).
2. UI toolkit for the real UI (the backend doesn't depend on it).
3. `same_passphrase = false` semantics (§8.6 states the assumed reading).
4. Arch with Secure Boot on: sbctl in Setup Mode (planned) vs. ask to disable SB.
5. Whether Borea installs an AUR helper when the user ticked AUR apps (default: no).
6. Debian + RTX 50: nouveau fallback (current) vs. steering the user to another distro on 03 (warning exists).

## 14. Definition of done

A person boots the ISO on any UEFI x86-64 machine, joins Wi-Fi, accepts or changes the suggested
region, picks one of four distros, a desktop (or none), an environment, add-ons and apps, a disk layout
and a name — and gets a booting, encrypted, online system with the right graphics driver, without
ever being asked about drivers, repos or partitions, and without anything being written before they
pressed Install.
