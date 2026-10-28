# Live image verification

2026-10-28, `codex/fast-live-boot`.

- All 65 selected build, early-boot, session and USB-helper tests pass after incorporating the latest graphical-target startup fixes. Clippy with warnings denied passes for those crates and the GUI.
- The actual `borea-live` binary starts inside cage with a headless Wayland output, pixman compositor and Mesa software rendering. It presents several frames, prints `BOREA_GUI_READY`, and exits successfully. The isolated Debian 13 runtime includes `libwayland-egl1`; this exercises real native window/context creation rather than mocked GUI rendering.
- Native Buildroot 2026.08.1 configuration accepts the defconfig, including `BR2_PACKAGE_CAGE=y`, Wayland/EGL and `BR2_TARGET_ROOTFS_EROFS_COMPRESSION_ALGORITHMS="lz4"`. Configuration completed in six seconds. This is **not an ISO build time**.
- Build-time guards reject invalid graphical-target startup links, missing session launcher, absent Wayland/EGL libraries, missing keymaps/PAM and a polkit without logind. Cache tests invalidate changed manifests/source and corrupted or interrupted archives.

Not yet verified: a complete rebuilt ISO, its raw-USB UEFI boot in QEMU, or a boot on the reported physical laptop. The native compositor check does not certify those paths. No two-minute end-to-end build/download/write time is certified; first source builds still compile the kernel and graphics stack. No prebuilt Borea ISO release was available when checked on this date.

## Download and automatic app setup

2026-10-28, `codex/download-and-auto-install`.

- All 56 selected USB-helper, planner and executor tests pass. This includes setup with missing and corrupt releases (no compiler, package manager, clone or unverified executable runs), a verified ready-made app install, and first-boot Flatpak plans for all four base distributions.
- Queue execution tests simulate a network failure after remote setup, verify that only unfinished work remains in a mode-0600 checkpoint, and successfully resume it. Invalid commands are rejected before any queue action executes.
- Formatting and clippy with warnings denied pass for those crates, the GUI and early boot.

The queue tests inject command results; they do not certify an actual Flatpak download on the installed systems. First-boot network/app installation in disposable VMs, GUI-to-backend selection mapping and a complete rebuilt ISO remain release gates. Native apps and AppImages still install before reboot; explicitly enabled AUR can compile packages. Download-only setup needs published artifacts, which are still unavailable at this check.
