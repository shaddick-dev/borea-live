# Implementation roadmap

Milestones have exit criteria instead of speculative calendar promises. The read-only foundations exist: catalog, host capability detection, hardware discovery, adapter contract, and `borea doctor`. None of these implementation gates has passed.

## M0 — Prove the boot and runtime assumptions

Build four small spikes:

1. A Rust Iced window running under tested X11/Wayland and VM graphics configurations, both in a Borea Live prototype and in a live session of another family.
2. A manually assembled Arch target disk booting via a pinned Limine release in QEMU/OVMF.
3. A native Arch kernel upgrade that regenerates the required initramfs/boot entries and still boots via Limine.
4. **Host independence:** assemble the Arch target using tools from the signed upstream bootstrap archive, run from a non-Arch live session (Fedora or Debian live) as well as from Arch. Then prove one acquisition strategy of each other kind on paper or in a VM: a container-image toolchain (Fedora or Tumbleweed), an image used as a source without booting it (PikaOS or Nobara), and Bazzite's image deployment.

Record exact Limine syntax, artifact paths, kernel protocol, host dependencies, package hooks, and VM settings. Evaluate the release metadata client and GUI accessibility requirements at this stage.

Also prove local progressive hardware discovery, NetworkManager Wi-Fi from the bootstrap TUI before the GUI is downloaded, and low-cost wind animation with reduced motion. Evaluate deployment/boot constraints for Bazzite and required storage/encryption for Omarchy early; do not defer discovering architectural incompatibilities until after their menus ship.

**Exit:** reproducible evidence for initial boot and post-upgrade boot, a proven GUI/runtime combination, and an Arch target built identically from two host families. Resolve the boot integration before building the entire wizard around assumptions.

## Borea Live track (starts in M0, runs alongside M1–M5)

Borea Live is the reference installation environment, not a late extra. It is a minimal neutral image with broad network firmware, NetworkManager, polkit, a Wayland compositor for Windpad, and the `borea` bootstrapper, and it carries an `/etc/borea-live` marker. Prototype it in M0 and use it as a test host from M3. Before the public alpha, publish it with signed checksums and test USB boot.

**Exit (by M5):** Windpad, Wi-Fi, and an Arch installation work from Borea Live on test hardware without network-firmware gaps on the documented adapters.

## M1 — Working bootstrapper

Implement the Rust workspace, versioned bundle format, release metadata verification, streaming download/cache layer, and Ratatui interface. Launch a harmless GUI shell from a verified test release.

**Exit:** a user can download, cancel, resume where supported, verify, and launch; corrupted bundles, unsigned/expired metadata, wrong-architecture bundles, and unsafe archives are rejected. Missing graphical dependencies produce actionable errors. No installer operation changes target disks.

## M2 — Read-only discovery and planning

Implement host inspection, block graph discovery, capability checks, install request/plan schemas, deterministic plan rendering, profile selection, and the GUI review flow. Use a fake executor for previews.

Build the minimal Windpad, family/distribution/edition dropdowns, hardware and Wi-Fi drawers, explainable driver suggestions, and signed upstream-specific latest-release resolution. Pin exact target identities in plans. Wi-Fi and hardware discovery are core scope, not deferred extras.

**Exit:** representative physical and virtual device fixtures resolve safely; active host disks and installation media are excluded; unsupported combinations cannot become executable plans; plan changes invalidate approval.

## M3 — Privileged execution and minimal Arch installation

Implement authenticated IPC, privilege elevation, protected verified worker launch, operation locking, target revalidation, partitioning, filesystem setup, native bootstrap, minimal target configuration, Limine deployment, and journals.

**Exit:** the minimal installation boots from a new virtual disk under UEFI, permits normal-user login, and provides working networking. Tests prove that path substitution, target changes, concurrent installs, and malformed IPC cannot bypass worker validation. Destructive tests operate only on disposable VM disks.

## M4 — Desktop profiles and recovery

Implement and independently certify GNOME, KDE Plasma, and Xfce profiles. Complete accurate progress, safe cancellation, retry semantics, redacted support bundles, target validation, and explicit reboot handling.

**Exit:** each advertised profile installs and reaches a usable desktop. Interrupted downloads, failed packages, full ESP, NVRAM failure, UI disconnect, and power interruption have tested outcomes and accurate user messages. Post-upgrade boot passes for every supported kernel/profile combination.

## M5 — Public Arch alpha/beta

Establish the real release host, signing ceremony, protected CI release flow, software bill of materials, license inventory, public install instructions, compatibility matrix, and issue templates.

Run limited hardware trials on spare disks with informed testers. Document firmware-specific issues and installation-host prerequisites.

**Exit:** published binaries can be obtained and verified using the documented path; supported installation tuples pass automated gates and documented hardware checks. Stable promotion follows release criteria, not a fixed date.

## M6 — Priority base adapters: Fedora and Debian

Implement Fedora and Debian with their native bootstrap, latest stable release resolution, security configuration, profile mappings, kernel hooks, and Limine integration. Preserve Fedora SELinux behavior and Debian's stable/security update policy. Arch resolves current rolling repositories. Revisit abstractions based on evidence from all three families.

**Exit:** all three bases install their current eligible release and pass first boot plus post-kernel-upgrade boot through the same workflow. A newer unqualified release is shown pending, with no automatic fallback to an older one.

## M7 — Requested derivatives and Tumbleweed Gaming + Firmware

Implement dedicated adapters and certify all requested editions:

- CachyOS Desktop and Handheld, preserving upstream desktop/device choices and optimized package eligibility.
- Omarchy, with required upstream layout/encryption and update behavior implemented before enabling its installation.
- Nobara Official, GNOME, KDE, Steam-HTPC, and Steam-Handheld, tracking current unified image/driver policy.
- PikaOS GNOME, KDE Plasma, Hyprland, Niri, and COSMIC, with every currently official Standard/NVIDIA variant and its CPU requirements.
- Bazzite's complete official hardware/desktop/Gaming Mode image matrix through a deployment-aware adapter, including tested native updates, rollback, and a certified explicit boot path. Official-media handoff alone does not meet this gate.
- Tumbleweed's latest coherent snapshot, with Gaming + Firmware installed into the target before completion and native update/boot behavior preserved.

**Exit:** every advertised supported edition passes installation, login, device/network setup, native updates, and reboot. Image targets also pass rollback. Latest-release freshness, unavailable/missing editions, hardware eligibility, and integration-pending states have automated coverage. Do not describe the full requested catalog as implemented while entries remain only menu placeholders.

## M8 — Broader capability tracks

Prioritize independently, with separate compatibility matrices:

- **Terminal installation:** a full TUI frontend over the existing planner/worker.
- **Advanced storage:** manual partitions, existing ESPs, dual boot, resizing, encryption, Btrfs, snapshots, hibernation, multi-disk/RAID/LVM.
- **More platforms:** Secure Boot, other architectures, legacy BIOS where justified.
- **Additional adapters beyond requested scope:** Alpine, Ubuntu, and a separate declarative-system design for NixOS.
- **Repeatability:** exported secret-free profiles, explicit noninteractive approval design, mirror snapshots, offline bundles.

Do not combine all of these into a first-release dependency.

Capability work required by a requested adapter, such as Omarchy's selected encryption/layout or Bazzite deployment semantics, is pulled into that adapter's gate rather than postponed under this optional list.

## Verification strategy

| Level | What it establishes |
| --- | --- |
| Pure unit tests | Capability resolution, plan invariants, config rendering, metadata and state-machine logic |
| Parser/property tests | Disk-topology edge cases, hostile metadata/archive inputs, bounded IPC parsing |
| Fake-executor integration | Event sequencing, plan revalidation, retry rules, secret redaction, cancellation boundaries |
| Local fixture server | Downloads, redirects, range semantics, interrupted transfers, stale/rotated metadata, integrity failures |
| Disposable QEMU/OVMF VMs | Actual partitioning, package installation, UEFI boot, native kernel upgrades, reboot |
| Fault injection | Disk full, network failure, interrupted package hooks, UI loss, power cuts at defined checkpoints |
| Hardware trials | Firmware NVRAM/fallback behavior, NVMe/SATA discovery, graphics, keyboard, network and firmware packages |
| Catalog freshness | Upstream latest resolution, independent derivative versions, edition retirement, metadata outage, no silent old-release fallback |
| Windpad interaction | Every requested dropdown, keyboard/touch operation, hardware partial results, Wi-Fi before download, reduced motion, theme, resize |
| Gaming/image targets | Tumbleweed preinstalled package/firmware manifest, GPU matching, Bazzite native update/rollback, derivative layout requirements |

Run normal Rust format, lint, and test checks for each implementation change. Gate destructive integration tests behind a dedicated VM harness that rejects host block-device paths. Use sparse image files as guest disks; never target the contributor's real system disk.

VM tests should verify boot success through a guest-side signal and inspect the installed system, rather than passing merely because QEMU starts. Include a kernel update and second boot. Keep proof artifacts: installer version, plan, adapter version, package manifest, VM configuration, serial logs, and test outcome, with secrets removed.

## Initial implementation backlog

1. Record architecture decisions for Iced, Limine version, Borea Live base and build tooling, worker privilege mechanism, and release trust library.
2. Build the Limine first-boot and kernel-upgrade spike.
3. Build the GUI/live-environment compatibility spike.
4. ~~Create the Cargo workspace and shared domain/error types.~~ Done: read-only foundations and `borea doctor`.
5. Define release metadata and bundle compatibility contracts.
6. Implement verified downloads and test against a local fixture server.
7. Implement TUI readiness, progress, verification, and launch.
8. Implement read-only block graph discovery and fixtures.
9. Define typed plans, capability resolution, and review rendering.
10. Design and test authenticated worker launch and IPC before enabling destructive operations.
11. Implement disposable-VM storage operations and the Arch adapter.
12. Wire the GUI through review, execution, recovery, and completion.
13. Certify Fedora and Debian latest-release adapters, then every named derivative and Tumbleweed Gaming + Firmware.

## Decisions still requiring evidence

- Exact GUI renderer and accessible-input support on the selected live environment.
- Exact pinned Limine release, config grammar, boot protocol, and update packaging.
- Whether boot maintenance can use native distro packages or requires a Borea-owned integration package.
- Release host and signing-key custody.
- Supported package-cache policy and minimum memory/disk requirements, derived from measured installs.
- Which desktop profiles are ready for the first advertised release.

These are implementation research items, not hidden promises. The architecture can be implemented incrementally while each is resolved at its milestone gate.
