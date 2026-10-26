# Disposable UEFI installation tests

These tests never attach a host block device. The Rust guest binary refuses to run without its explicit QEMU firmware marker and a target with serial `BOREA-TEST-TARGET`. The runner only attaches `/work/target.qcow2` (40 GiB sparse) and copied OVMF variables. Its public test password is a fixture, never a product default.

Build `borea-vm-test` and `borea-vm-runner` from the backend workspace, and build `borea-firstboot` for `x86_64-unknown-linux-musl`. Stage that static helper alongside the native test binaries; a dynamically linked target helper is rejected. Build this directory's Dockerfile as `borea-vm-test:kernel`. Prepare a disposable bind-mounted work directory with marker file `.borea-test-workdir` containing `BOREA_TEST_WORK`, copies of `init` as `test-init` and `borea-dhcp`, and the public `dist/keys/*.gpg` under `keyrings/`.

Inside the tools container, mount that directory at `/work`, backend binaries read-only at `/binaries`, and the host's public combined CA bundle read-only at `/run/borea-ca.pem`. Run `build-initrd.sh`; then `/binaries/borea-vm-runner` to install and `/binaries/borea-vm-runner boot` to boot the installed disk. Preserve the same OVMF variables between these commands. Set `BOREA_TEST_DISTRO=arch|debian|fedora|opensuse` and use a separate work directory per distribution. This environment has no KVM device, so local runs use slow TCG emulation.

No privileged container flag or host device bind is needed. The live guest trusts the session CA only in RAM via fw_cfg. The proxy is passed through an anonymous memfd, not a command-line credential or saved file. Do not bake session trust into an image layer. Public upstream verification keys are separate from session transport trust.

Installation logs are redacted. The boot console checks disk unlock, the account, distribution, network and sudo before powering off. A kernel panic, timeout, missing completion marker or failed assertion is a failing test. A successful planner matrix alone is not an installed-system test.

The whole-drive test entry point exists for all four distros. Alongside/Windows and enrolled Secure Boot fixture automation remain release gates recorded in `docs/VERIFIED.md`; they are not claimed as passing.

The qcow2 test drive uses QEMU `cache=unsafe` only to reduce fsync overhead under TCG. It is disposable and is not a power-loss durability test; production disk I/O is unchanged.
