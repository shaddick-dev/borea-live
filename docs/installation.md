# Installation and Limine lifecycle

## Supported initial storage model

The initial storage implementation targets x86-64 UEFI machines with Secure Boot disabled and one explicitly selected erasable disk.

This describes the first Arch vertical slice. The complete catalog uses adapter-declared layouts: preserve image-based deployments, required encryption, SELinux labeling, Btrfs/snapshot conventions, and native boot/update ownership where applicable. A derivative requiring unavailable storage or boot features stays pending until that integration is implemented; do not silently weaken its defaults.

| Partition | Proposed layout | Purpose |
| --- | --- | --- |
| EFI System Partition | 2 GiB, FAT32, mounted at `/boot` | Limine EFI executable, configuration, kernel and initramfs artifacts |
| Linux root | Remaining usable space, ext4, mounted at `/` | Base system, applications, user data |

Use GPT, proper alignment, standard partition type identifiers, and filesystem UUIDs in persistent configuration. The 2 GiB ESP is a proposed baseline; the planner calculates required space from boot artifacts, expected upgrade staging, retained kernels, and a reserve, then rejects insufficient layouts. Disk minimum size comes from the selected profile's estimate plus package/cache/workspace margins, rather than a single universal constant.

For the first release, creating a new ESP is part of erasing the selected disk. Reusing an existing ESP, preserving another OS's fallback loader, and handling multiple ESPs require a separate alongside-installation design.

Reject unsupported storage stacks and ambiguous ownership. Read-only discovery must trace partitions, device-mapper mappings, RAID members, loop devices, and active mounts to their backing devices. If the implementation cannot safely trace a stack, exclude its disks from erase eligibility.

Never select by a transient `/dev/sdX` name alone. Record the strongest available stable identity, size, topology, and device metadata, and re-resolve it before each destructive operation. Virtual disks may lack serial numbers; a supported fallback identity policy must be explicit and tested rather than silently assuming uniqueness.

## Execution stages

### Preflight and asset preparation

- Confirm the supported host, architecture, UEFI state, and Secure Boot status.
- Verify dependencies, network access, repository trust roots, target availability, and working space.
- Resolve the package set and authenticate every asset that will be used.
- Resolve the latest selected upstream release/edition, hardware-compatible image/driver branch, firmware and gaming package set; pin identities and freshness in the approved plan.
- Prefetch the complete resolved package closure before erasure where the adapter supports it and cache space permits.
- Otherwise disclose that installation remains network-dependent after erasure and include that fact in the approved plan. An online installer cannot promise success if mirrors fail mid-install.
- Record the approved plan digest, target identity, and operation journal outside the target before partitioning.

### Storage

- Acquire an installation lock and revalidate topology.
- Unmount only resources created or explicitly owned by the installation workflow; do not force-unmount unrelated host filesystems.
- Write GPT, create the ESP and root partition, wait for device discovery, format, and mount in a private mount namespace.
- Verify that all target writes land on the selected filesystems.

This phase is irreversible with respect to the old disk contents. The journal supports diagnosis and safe retries; it is not a backup or a guarantee of rollback.

### Base system and profile

- Bootstrap a minimal system using the adapter's native mechanism and authenticated repositories.
- Install the kernel, firmware policy, selected profile, display manager where applicable, network tools, and necessary system utilities.
- Use repository consistency and version-pinning mechanisms appropriate to that distribution. Do not promise a bit-for-bit identical rolling-release installation without a snapshot repository.
- Record package versions and repository identities for diagnosis.
- Install resolved CPU microcode, graphics/network/Bluetooth firmware and driver components before completion. Tumbleweed defaults to the reviewed Gaming + Firmware package set described in [hardware policy](hardware-network.md).

For image-based targets, the adapter instead deploys its verified native image using a certified deployment/layout path and validates its native update and rollback mechanisms. Do not approximate it with base-family packages.

### Target configuration

- Generate fstab with persistent identifiers and the approved mount options.
- Set locale, keyboard, time zone, hostname, and native network configuration.
- Create the normal user, establish administrator access, and apply the adapter's root-account policy.
- Set passwords through supported native interfaces without putting secrets in command arguments.
- Enable the correct networking and display-manager services through the distribution's conventions.
- Generate an initramfs appropriate to the installed kernel and selected filesystem.

### Limine setup

Limine is Borea's default installed bootloader. Pin a tested Limine release per adapter/release combination and render configuration for that exact configuration grammar. Do not mix examples from incompatible Limine versions.

For the wider catalog, certify the complete target boot/update lifecycle before offering Limine. Bazzite's image/deployment and rollback behavior requires separate integration research. A native-loader handoff, if supported, must be explicit in review and tested across upgrades/rollback. No silent bootloader replacement or assumption that one kernel entry serves every deployment model.

Proposed boot-file organization:

```text
/boot/
  EFI/
    Borea/                 # version-pinned Limine EFI binary
    BOOT/BOOTX64.EFI        # fallback path on the newly created ESP
  borea/
    kernels/<generation>/  # verified kernel/initramfs files if versioned staging is used
    metadata/              # installed boot-generation records
  <limine-config-location>  # exact supported location defined by pinned release
```

The final paths, config discovery rules, kernel protocol, initramfs/module syntax, and microcode handling must be proven against the selected Limine release in the implementation spike. The placeholder above intentionally avoids asserting an unverified config filename or syntax.

The boot integration must:

1. Resolve the installed kernel, initramfs, required microcode, and root UUID from the target.
2. Verify that the pinned Limine version supports the selected kernel boot protocol and artifact format.
3. Put all referenced files on filesystems readable by that boot path; the initial design uses the ESP for boot artifacts.
4. Render an entry using stable filesystem identifiers and validated artifact paths.
5. Deploy Limine on the new ESP and create a named UEFI boot entry.
6. Install the standard removable-media fallback path only as an explicit part of the approved new-ESP layout.
7. Detect NVRAM-write failures and use a tested fallback flow, reporting the limitation to the user.
8. Install distribution-native hooks that regenerate or stage boot entries when kernels and initramfs files change.

Secure Boot, legacy BIOS, aarch64, encrypted root, and measured boot are later support tracks. None is implicitly supported just because Limine or a Linux kernel can support parts of that configuration.

### Validation and completion

Before reporting success, verify mounted targets, fstab identifiers, kernel/initramfs existence, Limine configuration references, ESP free space, user setup, enabled services, and boot entry or fallback presence. Run any adapter-specific validation commands in the controlled target environment.

Sync and unmount safely. Explain whether firmware NVRAM or fallback boot will be used. The installer can validate files and configuration before reboot; a verified successful boot is established by VM/hardware testing, or by the user actually booting the installed system.

## Kernel updates are part of installation support

A successful first boot is insufficient. Each adapter must implement and test the package-manager hooks that keep Limine entries synchronized with kernel changes.

Where staged boot generations are used, build and validate new artifacts before switching the selected entry. Preserve a working boot generation until the new one is complete and enough free space remains. Define cleanup and retention policy explicitly.

FAT filesystems do not provide a power-loss-safe multi-file transaction merely because individual files are renamed. Write ordering, flushes, fallback generations, and power interruption tests determine recoverability. Do not market this as transactional boot without evidence.

Where an adapter instead relies on stable kernel filenames, document the resulting recovery limits and test upgrade interruption. A stable path does not by itself retain a previous bootable kernel.

Borea may install a small adapter-owned boot-maintenance package with native hooks. It should not require a persistent Borea daemon or the GUI to maintain the system. The package's ownership, updates, removal behavior, and relationship to distro packaging are part of adapter support.

## Secure Boot (opt-in)

Secure Boot is **off by default**. The installer offers it on the Review step, beside the machine's current state ("Secure Boot off · Setup Mode no"). Every step is listed before the user can approve. When chosen, Borea uses the machine owner's own keys:

1. Create this machine's own PK, KEK, and db keys on the new system, readable by root only.
2. Sign Limine, and enroll the BLAKE2b hash of its config into the Limine executable. Every kernel and initramfs path then carries its hash. Under Secure Boot, Limine refuses any file that doesn't match and disables its config editor. The adapter's kernel-update hook renews the hashes and the config enrollment on every kernel update.
3. Keep Microsoft's keys enrolled alongside, so graphics and network option ROMs still load. Removing them can leave a machine with no display.
4. Back up the firmware's current PK, KEK, db, and dbx to the EFI partition before changing anything.
5. Never flip the firmware's own Secure Boot switch. If the firmware is in Setup Mode, Borea enrolls the keys itself. Otherwise it leaves its certificate on the EFI partition to enroll from firmware setup. After the user turns Secure Boot on, Borea checks the first boot actually ran with it enforced.

Safety rules in `borea_distro::secure_boot`:

- It is unavailable on legacy BIOS, 32-bit UEFI, or unreadable firmware.
- It is unavailable for targets whose boot path is not certified with it, currently Bazzite's image deployment.
- If the user changes to such a target, the choice drops back to off.
- If firmware Secure Boot is on but the user leaves it off for the new system, the review warns that the new system will not boot until Secure Boot is turned off in firmware.

Execution of these steps arrives with the privileged worker. The choice, its checks, and its review rows exist now.

## Failure and resume policy

| Failure point | Intended behavior |
| --- | --- |
| Release or package download before erasure | Retry verified download or choose another approved mirror |
| Target identity/topology changed | Stop and require a fresh plan and review |
| Partitioning or formatting fails | Stop; diagnose actual on-disk state; do not blindly replay destructive operations |
| Package installation interrupted | Adapter evaluates native package state and offers a documented recovery or clean reinstall |
| Initramfs or boot setup fails | Preserve target and logs; retry only after verifying mounts, artifacts, and available space |
| GUI disconnects | Worker completes a critical operation, then stops at a safe boundary; permit authenticated reattachment if implemented |
| Power is lost | Rediscover and inspect; never assume the last journal write proves disk or package state |

Only repeat operations with verified preconditions and explicit retry semantics. The first release may support stage-local retries without supporting full cross-reboot resume. The UI and documentation must distinguish these capabilities.
