# borea-catalog

The model behind Borea's software step (page 05).

- **Traditional install groups** (`src/groups.rs`): one base environment (Minimal, Server, Server with GUI,
  Workstation) plus any add-ons, in real-world sections: Server roles (Web, Database, File Sharing),
  Virtualization (VM host, Containers), Tools (Development, Monitoring, Network, Gaming Tweaks).
  Every package can be ticked or unticked (`Picks`) except the five a system needs to boot; extras start
  unticked. Add-ons can also bring a desktop app that belongs to the role (File Sharing → LocalSend, from
  Flathub or the AUR) and open the firewall for what they install (`FIREWALL`). Written once with
  distro-neutral keys, mapped to each distro's real names.

- **Desktop apps only** in the app list (`src/catalog.rs`), in app-store categories: Browsers, Chat & email,
  Office, Audio & video, Graphics, Games, Development, Utilities. Command-line tools live in add-ons; a test
  makes sure nothing is in both.
- **Sources are only the chosen distro's repos**, plus the AUR on Arch, Flathub, and upstream AppImages
  (`src/source.rs`). Another distro's repos can't be switched on; the AUR can't be switched on anywhere
  but Arch. An AppImage is the last resort: only for apps with no packaged source here (e.g. Helium off
  Arch), fetched from the app's own GitHub release; no vendor repo is ever added.
- **Install from:** distro packages first, or Flathub first (`Prefer`), like WinUtil's winget/Chocolatey switch.
- **Every ticked app resolves to exactly one source.** Ticking an app whose only source is off switches
  that source on, so a ticked box never silently installs nothing (`Selection::toggle`).
- **Drivers are automatic** (`src/hardware.rs`, `src/drivers.rs`). The live ISO reads DMI, PCI and sysfs:
  maker and model, laptop or desktop, every GPU and its generation, which GPU drives the built-in screen,
  a GPU MUX (ASUS `gpu_mux_mode`, or inferred from the panel), Secure Boot, CPU vendor, Intel audio DSP,
  Broadcom Wi-Fi and fingerprint readers. `plan_drivers` then picks per distro:
  NVIDIA Turing and newer → open modules; Maxwell/Pascal/Volta → the 580 legacy branch (Debian: 550);
  Kepler and older → nouveau; AMD and Intel → Mesa with the right video-decode driver; hybrid laptops →
  PRIME offload with switcheroo-control and NVIDIA power services; plus microcode and firmware.
  Sources the drivers need (RPM Fusion nonfree, openSUSE's NVIDIA repo, the AUR for 580xx) are switched
  on and locked. Self-signed modules with Secure Boot on mean a one-time key approval on first restart.
- `plan()` groups the picks into one batch per source for the installer to run, and reports
  any app stranded by a source switched off afterwards.

```sh
cargo test                                  # 31 tests
cargo run --example export_json > catalog.json   # data used by the design prototype
```

Package names and repo placement were written from knowledge of each distro, not queried live,
and sizes are rough. Before shipping, regenerate the table from each distro's package index and
Flathub's appstream data.
