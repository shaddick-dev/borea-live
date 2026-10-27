# Borea Rust online installation backend

Independent Rust 2024 workspace, MSRV 1.85. The existing design/GUI workspace remains available at the repository root. This backend does not embed distribution payloads in the live ISO: it verifies and downloads the selected upstream system online.

Targets: Arch Linux rolling, Debian 13/trixie, explicit Fedora stable release, openSUSE Tumbleweed. UEFI x86_64; encrypted LUKS2/btrfs; whole drive or alongside an existing healthy Windows NTFS installation. Profiles, add-ons, apps and automatic drivers share `borea-catalog`.

## Build and check

```sh
cd backend
cargo build --locked --release --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Pure libraries compile without root or access to real disks. Tests use generated disk and hardware facts. Never run the installer on a development host. The explicitly marked QEMU test guest is the place for destructive integration tests.

## Live environment integration

Install `borea-installd`, `borea-cli`, `borea-firstboot` and `borea-probe` into `/usr/bin`. Build `borea-firstboot` statically with `cargo build --release --target x86_64-unknown-linux-musl -p borea-exec --bin borea-firstboot` (Rust target and musl C toolchain required). Its ELF interpreter check refuses a live-libc-dependent helper before partitioning. Install the service/sysusers/tmpfiles definitions from `dist/`, the public native verification keyrings into `/usr/share/keyrings/borea/`, and `debian-archive-keyring` into its native path. Provide the live `borea` user and NetworkManager system bus. The GUI talks to the restricted Unix socket and never runs as root.

The live image needs a current kernel/firmware plus `debootstrap`, `cryptsetup`, btrfs/ext4/FAT/NTFS tools, `sgdisk`, `sfdisk`, `partprobe`, util-linux, udev, `efibootmgr`, GnuPG including `gpgv` (a separate Debian package), OpenSSL, tar/xz/zstd, skopeo/umoci (container tool roots), and trusted HTTPS CA certificates. Native distro tooling runs in a verified temporary root under `/run/borea/tools/`; the live image's own distribution does not choose the installed distribution.

`/etc/borea/bootstrap.json` can contain a reviewed array of `Bootstrap` manifests. Without it, the service resolves current upstream artifacts. Native metadata and package checks happen before partitioning. A source or signing failure blocks installation; signature verification is never disabled.

Review returns an approval digest covering the actual job, geometry, firmware facts, bootstrap and pinned external sources. Installation requires that digest and separately supplied credentials. The privileged executor regenerates the job, isolates mounts and rechecks disk identity immediately before writing. Secrets never belong in a plan, shell argument or source archive.

```sh
borea-cli probe
borea-cli disks
borea-cli review --plan my-plan.toml
borea-cli render --plan my-plan.toml
borea-cli install --plan my-plan.toml --approval REVIEW_DIGEST < /dev/stdin
borea-cli events 0
```

The install command reads one credentials JSON object from stdin; use an interactive credential collector or protected pipe. The service API is documented in [docs/PROTOCOL.md](docs/PROTOCOL.md). Recovery is limited to explicitly retryable native package actions; partitioning cannot be replayed automatically.

## Verification status

See [docs/VERIFIED.md](docs/VERIFIED.md) for evidence and outstanding release gates. Compiling a pathway is not evidence that its installed OS boots. This workspace must not be advertised as a certified installer until its native package and QEMU Secure Boot/alongside matrices pass. Existing preview UI install guards remain intact.
