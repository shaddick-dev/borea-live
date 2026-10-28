# Compose Borea Live without a release

From an x86-64 Linux checkout, with Rust 1.95+ and access to a Linux Docker daemon:

```sh
cargo run --locked --profile app -p borea-flash -- --compose
```

The updated helper also offers **b — compose ISO** after a failed release download
when it can find this checkout. To create the image without opening the USB writer:

```sh
cargo run --locked --profile app -p borea-build -- compose
```

Output: `target/live/out/borea-live-compose.iso` and its `.sha256`. The writer still
requires typed device confirmation and verifies every byte. Composition itself
never accesses host block devices or requires a privileged container.

## How it avoids the long build

1. Docker downloads authenticated upstream packages for the live runtime: kernel, Mesa,
   Wi-Fi/GPU firmware, systemd, cage, NetworkManager and native installation tools.
   Debian 13 packages supply these components; the ISO is assembled locally with
   Borea's own boot and session. This does not select Debian for the installed OS.
2. The host's Rust toolchain compiles Borea inside that runtime so its programs
   match the live system's ABI. The target first-boot helper is statically linked.
   The Cargo registry and compiled dependencies are reused on later runs.
3. Pinned, SHA-256-checked upstream Limine binaries supply the boot stages. Only
   its small host installation utility is compiled, without building LLVM.
4. Rust configures the live user and services, validates the session files, packs
   EROFS with LZ4, and makes the hybrid BIOS/UEFI ISO. Its early boot includes the
   prebuilt kernel's required storage/filesystem modules and read-only loop fallback.

Arch, Fedora, Debian and openSUSE adapters remain independent of the live host.
The GUI remains a preview: composing the ISO does not complete its pending backend
integration or certify every target install. Secure Boot is off for composition;
the separate Buildroot path supports custom signing. BIOS live boot does not add
legacy-BIOS target installation support.

The normal boot uses software rendering. Hardware acceleration is an explicit
boot-menu option; a stalled GPU can otherwise leave a compositor running with a
blank screen. Overlay directories/files get explicit 0755/0644 permissions,
independent of the source checkout's umask, so D-Bus and the user session can start.

The first run downloads the runtime and compiles Borea's apps, so it is slower than
a cached composition. Network, CPU, storage and Docker's storage driver affect time;
there is no guaranteed two-minute cold build. Keep `target/live/compose`, the Cargo
cache and Docker's runtime image to retain the speedup. `CARGO_BUILD_JOBS` controls
Rust parallelism (otherwise up to four jobs).

## Maintainer options

```sh
cargo run --profile app -p borea-build -- compose --refresh-runtime
cargo run --profile app -p borea-build -- compose --ca-cert /path/to/build-ca-bundle.pem
cargo run --profile app -p borea-build -- compose --bin-dir /path/to/compatible-binaries
```

`--refresh-runtime` downloads current packages instead of reusing the Docker layer.
`--ca-cert` mounts a build-only CA bundle; it is excluded from the ISO. `--bin-dir`
skips Rust compilation when compatible x86-64 binaries already exist: `borea-live`,
`borea`, `borea-flash`, `borea-installd`, `borea-cli`, `borea-init`, `borea-session`,
`borea-build`, and a static `borea-firstboot`. It is intended for validated build
pipelines. Ordinary users should let the composer build matching binaries.

The **Compose Borea Live without a release** GitHub workflow exercises the normal
path, boots a disposable copy as a read-only UEFI USB device, requires native GUI
frames, and uploads the ISO and checksum as a seven-day Actions artifact. Publishing
a GitHub release is not required. No artifact is labelled as a tested release
merely because its build succeeded.
