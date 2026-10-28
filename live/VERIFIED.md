# Live image verification

2026-10-28, `codex/fast-live-boot`.

- 62 selected build, early-boot, session and USB-helper tests pass. After the final runtime-library guard, all 23 build/early-boot/session tests pass again. Clippy with warnings denied passes for those crates and the GUI.
- The actual `borea-live` binary starts inside cage with a headless Wayland output, pixman compositor and Mesa software rendering. It presents several frames, prints `BOREA_GUI_READY`, and exits successfully. The isolated Debian 13 runtime includes `libwayland-egl1`; this exercises real native window/context creation rather than mocked GUI rendering.
- Native Buildroot 2026.08.1 configuration accepts the defconfig, including `BR2_PACKAGE_CAGE=y`, Wayland/EGL and `BR2_TARGET_ROOTFS_EROFS_COMPRESSION_ALGORITHMS="lz4"`. Configuration completed in six seconds. This is **not an ISO build time**.
- Build-time guards reject a missing/wrong display-manager alias, missing session launcher, absent Wayland/EGL libraries, missing keymaps/PAM and a polkit without logind. Cache tests invalidate changed manifests/source and corrupted or interrupted archives.

Not yet verified: a complete rebuilt ISO, its raw-USB UEFI boot in QEMU, or a boot on the reported physical laptop. The native compositor check does not certify those paths. No two-minute end-to-end build/download/write time is certified; first source builds still compile the kernel and graphics stack. No prebuilt BOREA ISO release was available when checked on this date.
