# Live image verification

2026-10-28, `codex/fast-live-boot`.

- All 65 selected build, early-boot, session and USB-helper tests pass after incorporating the latest graphical-target startup fixes. Clippy with warnings denied passes for those crates and the GUI.
- The actual `borea-live` binary starts inside cage with a headless Wayland output, pixman compositor and Mesa software rendering. It presents several frames, prints `BOREA_GUI_READY`, and exits successfully. The isolated Debian 13 runtime includes `libwayland-egl1`; this exercises real native window/context creation rather than mocked GUI rendering.
- Native Buildroot 2026.08.1 configuration accepts the defconfig, including `BR2_PACKAGE_CAGE=y`, Wayland/EGL and `BR2_TARGET_ROOTFS_EROFS_COMPRESSION_ALGORITHMS="lz4"`. Configuration completed in six seconds. This is **not an ISO build time**.
- Build-time guards reject invalid graphical-target startup links, missing session launcher, absent Wayland/EGL libraries, missing keymaps/PAM and a polkit without logind. Cache tests invalidate changed manifests/source and corrupted or interrupted archives.

At this stage the full Buildroot source image and physical laptop boot were unverified. The later composition verification below covers a separately assembled image. No two-minute cold end-to-end build/download/write time is certified; first Buildroot source builds still compile the kernel and graphics stack. No prebuilt Borea ISO release was available when checked on this date.

## Download and automatic app setup

2026-10-28, `codex/download-and-auto-install`.

- All 56 selected USB-helper, planner and executor tests pass. This includes setup with missing and corrupt releases (no compiler, package manager, clone or unverified executable runs), a verified ready-made app install, and first-boot Flatpak plans for all four base distributions.
- Queue execution tests simulate a network failure after remote setup, verify that only unfinished work remains in a mode-0600 checkpoint, and successfully resume it. Invalid commands are rejected before any queue action executes.
- Formatting and clippy with warnings denied pass for those crates, the GUI and early boot.

The queue tests inject command results; they do not certify an actual Flatpak download on the installed systems. First-boot network/app installation in disposable VMs, GUI-to-backend selection mapping and a complete rebuilt ISO remain release gates. Native apps and AppImages still install before reboot; explicitly enabled AUR can compile packages. Download-only setup needs published artifacts, which are still unavailable at this check.

## Local composition without a release

2026-10-28, `codex/compose-live-iso`.

- A 711 MiB ISO was composed from authenticated prebuilt runtime packages and compatible Borea binaries, with no Borea release. The final cached-runtime composition took approximately 72 seconds. This excludes compiling those Borea binaries and the first runtime download; the runtime preparation itself took about 2.5 minutes. It is not a two-minute cold-build claim.
- The unmodified ISO booted as a read-only emulated USB storage device through OVMF UEFI in QEMU/TCG. The normal boot entry displayed the actual installer automatically, without console repairs. The prebuilt kernel was 6.12.113+deb13-amd64. The hardware renderer had left the virtual display blank; software rendering is now the normal path, with hardware explicitly selectable.
- Overlay installation normalizes public directory/file modes to 0755/0644. A private checkout previously made /etc inaccessible to D-Bus; a regression test covers that failure. Early boot packs the prebuilt kernel's filesystem/storage modules, with read-only loop fallback for EROFS.
- Selected builder, init/session and USB-helper tests pass, as do formatting and clippy with warnings denied. The composition workflow builds Borea inside the runtime and requires a disposable UEFI USB boot to present native GUI frames before uploading its ISO artifact.

The local timed composition used `--bin-dir`, not a cold build of every Rust dependency inside Docker. The new workflow covers that normal source-compilation path. Physical USB hardware, the reported laptop, legacy BIOS live boot and actual distro installation from this image remain unverified. The GUI's backend integration remains pending. The artifact is a preview, not an installer release certification.
