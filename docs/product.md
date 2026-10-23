# Product and user experience

## Positioning

**Borea — Go wherever the wind takes you.**

Windpad: a powerful, beautifully minimal landing pad for the Linux system you choose.

Borea provides a common installation experience across supported distributions. It is an installer and bootstrapper, rather than a new distribution or replacement package manager. After installation, the user maintains the machine using their distribution's normal tools.

Primary audiences:

- Linux newcomers who want clear choices and explanations.
- Experienced users who want a fast, repeatable base installation.
- People testing distributions on spare machines or virtual machines.
- Later: maintainers who want to publish a reviewed Borea adapter for their distribution.

An online installer needs a booted environment with network access. Borea is independent of any distribution's ISO. Users start from **Borea Live**, Borea's own neutral live environment, or from any compatible live session. Borea never asks the user to boot the chosen distribution's ISO. A downloaded TUI cannot boot an otherwise empty machine by itself, so Borea Live is built early. See [the vision](vision.md).

## End-to-end journey

### 1. Launch the TUI

The user obtains the small `borea` executable from an official release channel and runs it in a terminal. The interface displays:

- Machine architecture, boot mode, memory, available download space, and network status.
- Whether the current environment is supported for the selected installer release.
- Installer version, channel, download size, and verification status.
- Actions: **Download and launch**, **Download only**, **Diagnostics**, and **Exit**.

The TUI offers integrated Wi-Fi setup before downloading the GUI. It then downloads the release bundle, verifies it, checks runtime compatibility, and launches the GUI as the current user. No disk changes occur during bootstrap. The installer is not automatically run as root when the bootstrapper was launched with elevated privileges; launch must establish the intended non-root session or stop with instructions.

If there is no graphical session, the TUI explains how to use a supported live desktop or saves the verified bundle for later. A terminal installation frontend can reuse the engine in a later release. Download progress alone must not imply that terminal installation is already supported.

### 2. Welcome and machine readiness

Open on Windpad: Borea wordmark, wind tagline, Wi-Fi button, compact hardware strip, and four distribution-family cards. Begin a local read-only scan automatically, publishing results as they arrive. Show readiness for networking, boot mode, Secure Boot, memory, runtime dependencies, and installation environment. Detected hardware is not a promise of target-driver compatibility.

Offer language and keyboard choices early. Integrated Wi-Fi is a core feature of both frontends, using the installation environment's NetworkManager service (always present in Borea Live). Show networks, signal, security, and connection state; support password entry and hidden networks. Distinguish Wi-Fi association from working internet access. See [hardware and networking](hardware-network.md).

Block unsupported installation combinations before asking the user to configure the rest of the system.

### 3. Choose a distribution

Show four family cards, each with a distribution dropdown and an edition dropdown where applicable:

- **Arch:** Arch Linux; CachyOS → KDE Plasma (upstream "Desktop") / KDE Plasma · Handheld / Minimal; Omarchy.
- **Fedora:** Fedora Linux; Bazzite → all official hardware/desktop/Gaming Mode choices; Nobara → KDE Plasma · Official / GNOME / KDE Plasma / KDE Plasma · Steam HTPC / KDE Plasma · Steam Handheld (upstream Official, GNOME, KDE, Steam-HTPC, Steam-Handheld).
- **Debian:** Debian; PikaOS → GNOME / KDE Plasma / Hyprland / Niri / COSMIC, each with the currently published Standard or NVIDIA variant.
- **openSUSE:** Tumbleweed with Gaming + Firmware enabled by default.

Edition lists come from reviewed, signed catalog data and track upstream changes. Show exact release identity, architecture, release model, support status, and a plain-language description. Only validated combinations can install in the stable channel. Pending entries remain visible with an explanation and no install action.

Default to **Latest**: current Arch repositories, latest stable Fedora/Debian, and each derivative's own current channel. Resolve authenticated metadata when connected, display freshness, and pin the result in the approved plan. If the latest release lacks certification, show **Latest release · integration pending** rather than silently selecting an older version.

Do not present experimental adapters as supported. Avoid subjective rankings such as "best distribution." Explain concrete differences such as rolling versus versioned releases.

### 4. Choose an environment

Offer Minimal, GNOME, KDE Plasma, and Xfce for base adapters where supported. Derivative editions retain upstream defaults. Show approximate download and installed sizes, with clear labels that these are estimates.

Explain hardware-based image/driver suggestions and allow eligible overrides. Unknown hardware stays unknown; hybrid laptops retain both GPUs in the plan. No storage target is automatically selected.

Tumbleweed's default Gaming + Firmware profile installs its resolved gaming stack, graphics libraries, controller support, device firmware, and CPU microcode before completion. Show exact packages and sources in review. Device firmware flashing is a separate task.

Each profile is a distribution-specific package mapping. A desktop choice also configures the corresponding display manager and required services; it is more than installing one package name.

### 5. Select storage

Show disk model, capacity, stable device identity, current partitions, and whether the device contains mounted filesystems or the installation media.

The first release supports **Erase selected disk and install**. Never preselect a disk. Exclude the live medium, current root disk, and disks used by the installer, package cache, or other mounted filesystems. Devices that cannot be identified safely are not eligible.

Later modes include guided alongside installation, manual partitioning, LUKS2, Btrfs layouts, and multi-disk setups. These require separate design and test gates.

### 6. Configure the system

- Locale and keyboard layout.
- Time zone.
- Hostname.
- Normal user name and password.
- Adapter-defined administrator access; default to a locked root account and explicit administrator group/sudo policy where supported.
- Optional swapfile, with hibernation excluded from the initial support contract.

Validate names and settings immediately. Passwords never appear in exported plans or logs. Explain which settings the selected distribution controls.

### 7. Review and approve

Display the exact distribution/release/edition, image/driver branch, firmware and gaming packages, repositories, disk identity, partition layout, filesystems, boot path, download estimate, and user settings. Mark the erase operation prominently. Adapters can require different layouts from the initial ext4 slice; an image-based target keeps its native deployment model.

Require the user to type a short confirmation derived from the selected disk identity, then approve the exact plan. A storage change or plan change invalidates approval and returns to review. Privilege elevation happens at execution, not when the user first opens the GUI.

### 8. Install

Display named stages and stage-specific progress:

1. Verify and prepare installation assets.
2. Recheck the selected disk and apply the storage layout.
3. Install the base system and selected packages.
4. Configure the system and user account.
5. Generate boot artifacts and install Limine.
6. Validate the installed system and finish.

Show real bytes for downloads and real package counts where available. For work that cannot be measured, show an activity indicator rather than invented percentages. Expandable logs offer detail without exposing credentials.

Cancellation is available at safe boundaries. During a critical disk or package operation, show **Stop after this step** with a precise explanation. After formatting, cancellation cannot restore the previous disk contents.

### 9. Finish or recover

Success shows the installed distribution, target disk, boot entry, and whether final checks passed. Reboot is an explicit user action after sync and unmount succeed.

Failure shows the failed stage, a useful explanation, what has already changed, and the available actions: retry a safe operation, inspect logs, export a redacted support bundle, or return to the live environment. Offer to save logs to an external location because a live session may be ephemeral.

## Visual direction

Use a Nord palette: snow-storm whites, polar-night greys, and a restrained frost-blue accent with generous whitespace. Fine animated airflow lines move behind the hero while cards and form controls remain still. Short directional transitions guide navigation. Keep one clear next action, reveal advanced controls on demand, and retain hardware/Wi-Fi access throughout. see the [Windpad design](windpad.md) for motion, layout, and interaction details. Use plain labels for critical actions.

Support light/dark themes, scalable text, keyboard navigation, visible focus, high contrast, and reduced motion. Validate screen-reader support on the selected GUI backend before claiming accessibility. Use icons alongside text, never as the sole indication of risk or progress.

## Product boundaries

Borea does not promise arbitrary distro support, transparent migration of an existing OS, safe resizing of every filesystem, guaranteed preservation after an erase operation, or zero-touch installation in its first release.

Telemetry is off by default. Local diagnostics are useful without a network account. Sending a support bundle is a separate user action with a preview of its contents.

Success means a user can install a supported distribution, boot it, connect to the network, log in, update its kernel using native tools, and boot again without needing Borea installed on the target as a background service.
