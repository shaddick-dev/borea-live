# Distribution catalog, latest releases, and adapters

## Required catalog and menus

Four family cards contain distribution dropdowns, followed by edition and hardware-specific controls where applicable. Family grouping is navigation: each derivative retains its own upstream release and deployment model.

| Card | Distribution | Required editions | Release policy |
| --- | --- | --- | --- |
| Arch | Arch Linux | Supported base desktop/minimal profiles | Current rolling repositories and current installation assets |
| Arch | CachyOS | KDE Plasma (upstream Desktop) and KDE Plasma · Handheld; Minimal (upstream No Desktop) | Current official rolling channel and edition assets |
| Arch | Omarchy | Upstream Omarchy experience | Latest official stable release and supported Arch integration |
| Fedora | Fedora Linux | Workstation, KDE Plasma, additional certified official profiles | Latest general-availability stable release and current updates |
| Fedora | Bazzite | All current official picker choices, including desktop, GPU/device, handheld/HTPC and Gaming Mode | Current upstream stable image channel, pinned by digest |
| Fedora | Nobara | KDE Plasma · Official, GNOME, KDE Plasma, KDE Plasma · Steam HTPC, KDE Plasma · Steam Handheld | Latest Nobara release and updates, independently of Fedora's version |
| Debian | Debian | Supported base desktop/minimal profiles | Latest stable release/point-release assets, security and stable updates |
| Debian | PikaOS | GNOME, KDE Plasma, Hyprland, Niri, COSMIC; Standard and NVIDIA for each where published | Current official rolling channel and edition assets |
| openSUSE | Tumbleweed | Gaming + Firmware enabled by default, supported desktop choice | Latest coherent published snapshot |

All requested editions belong in the catalog. New upstream editions become reviewed candidates and must pass certification before installation is enabled. Retired editions stop being offered for new installations. Do not invent image URLs or assume every desktop/GPU combination exists.

Research checked on 2026-10-22: CachyOS publishes Desktop and Handheld; Nobara lists five editions with unified standard/NVIDIA images; PikaOS lists five desktops with Standard and NVIDIA images. Bazzite uses a hardware-dependent image picker. These observations are not proof of Borea integration.

## Latest-release resolution

Latest is resolved when preparing an install, never compiled permanently into a GUI version label.

1. Refresh the signed Borea catalog and authoritative upstream release/repository metadata using the selected resolver.
2. Select latest stable/GA for Fedora and Debian; select the current coherent official channel/snapshot for rolling distributions. Fedora beta/Rawhide and Debian testing/unstable are not the default.
3. Resolve derivatives independently. PikaOS's own rolling integration is not replaced by Debian stable because it appears under that family card.
4. Check architecture, CPU instruction requirements, hardware/image compatibility, firmware mode, required layout, and certification for the exact release.
5. Show exact release/build/snapshot identity and check time. Image publication date and installed package state are distinct.
6. Pin hashes, image digests, repository metadata identities, catalog/adapter versions, and package policy in the approved plan. Do not follow moving latest references during execution.
7. Recheck freshness before destructive work. A changed plan requires renewed review; the engine never silently swaps in newly published assets.

If current metadata is unreachable, show cached identity/age and offer retry. Do not claim cached metadata establishes the latest release. If the newest release is not yet certified, show **Latest release · integration pending** and disable installation. An older supported release can be an explicit advanced choice, never an automatic fallback.

Rolling repositories are not necessarily immutable snapshots. Record package and metadata identities and maintain a coherent transaction policy. If required packages disappear or resolution changes, stop and resolve a new plan instead of mixing generations. Do not assume Tumbleweed retains every historical snapshot indefinitely; the pinned snapshot must actually remain obtainable.

## Adapter contract

The common engine owns the workflow. Each reviewed adapter owns:

- Release discovery, freshness, repositories/images, and upstream trust roots.
- Official editions and hardware eligibility, including CPU ISA and GPU-generation requirements.
- Native bootstrap or image deployment, transactions, and required storage layout.
- Distribution security defaults, account/network configuration, init system, services, and desktop.
- Native mappings for drivers, firmware, microcode, and gaming components.
- Kernel/initramfs integration, Limine/boot lifecycle, update hooks, rollback semantics, and cleanup.
- Validation, recovery boundaries, integration ownership, and maintenance-package behavior.

Adapters use reviewed Rust code and typed operations. Each declares its acquisition strategy and the host capabilities it needs (`crates/borea-distro`). A derivative is more than extra repositories added to its parent. Omarchy's upstream requirements, CachyOS's optimized packages, Nobara's changes, PikaOS's repositories, and Bazzite's image deployment each need dedicated integration.

## Native deployment and Limine

Arch, Fedora, and Debian are priority base adapters. An Arch vertical slice proves the engine but does not fulfill the entire catalog. Every named derivative and Tumbleweed Gaming + Firmware has an explicit roadmap gate.

Limine remains preferred. Every enabled integration must pass first boot and post-kernel-update boot. Preserve Fedora SELinux labeling, Omarchy's selected upstream layout/encryption requirements, Tumbleweed's filesystem/update behavior, and each target's other required security configuration. Those requirements can expand the storage capabilities beyond the initial Arch ext4 slice.

Bazzite needs an image/deployment-aware adapter preserving native updates and rollback. Do not use a generic DNF install-root operation or flatten its layout into the initial ext4 scheme. A Limine entry or native-loader handoff, if pursued, requires its own tested contract and must be visible in review. Until compatible boot/update/rollback integration passes, keep its install action pending. A separately labeled official-media download/upstream-install workflow may be useful, but it is not a completed Borea installation.

Host support and target support are separate matrices. Hosts are certified as capability profiles, not distributions. Borea Live is the reference host for the complete Windpad experience, and any live session with the required capabilities can install any target. Adapters bring the target's tools from upstream; see [the vision](vision.md).

## Certification and maintenance

Support identifies installer, catalog, adapter, host, target release/edition, CPU/architecture constraints, firmware mode, layout, GPU variant, kernel/initramfs, and boot/deployment versions.

Entries have planned, integration-pending, experimental, or supported status. Only supported combinations execute in the stable channel. Menu visibility never implies installation readiness.

Certify installation, normal-user/desktop login, network and driver/firmware operation, native updates, kernel upgrade/reboot, boot-space handling, recovery, and Borea-owned maintenance-package updates/removal. Image-based targets additionally require native update and rollback tests. Recheck rolling/latest metadata regularly and review new edition promotions.

Installed systems use native update tools without depending on a running Borea daemon or online catalog.

## Primary sources

Reviewed 2026-10-22. Runtime still resolves versions and editions dynamically.

- [Arch downloads and release model](https://archlinux.org/download/)
- [Fedora official releases and editions](https://fedoraproject.org/)
- [Debian release channels](https://www.debian.org/releases/)
- [CachyOS editions](https://www.cachyos.org/download/)
- [Omarchy installation requirements](https://omarchy.org/manual/getting-started/)
- [Bazzite image selection and installation](https://docs.bazzite.gg/General/Installation_Guide/install-guide/)
- [Nobara edition downloads](https://nobaraproject.org/download.html)
- [PikaOS desktops and GPU variants](https://pika-os.com/)
- [Tumbleweed installer and repositories](https://doc.opensuse.org/documentation/tumbleweed/yast_installer/)
