# Automatic hardware, Wi-Fi, firmware, and gaming

## Automatic local scan

Start a read-only hardware scan when the GUI opens; collect bootstrap-relevant facts in the TUI. Scan locally without uploading hardware or requiring an account. Use bounded asynchronous probes, progressive results, subsystem-specific errors, and hotplug invalidation.

| Subsystem | Facts | Purpose |
| --- | --- | --- |
| CPU | Architecture, model, instruction sets, vendor, cores | Image eligibility, optimized package requirements, microcode |
| Graphics | All PCI GPU IDs, models, drivers, hybrid configuration, available Vulkan data | Image selection, target drivers, Gaming Mode eligibility |
| Memory | Installed and available RAM | Readiness and cache strategy |
| Storage | Capacity, stable identity, topology, mounts, live-media backing disk | Eligible targets and storage review |
| Networking | Ethernet/Wi-Fi/Bluetooth devices, driver/firmware state, link, radio blocks | Wi-Fi setup and target firmware |
| Platform | UEFI, Secure Boot, model, virtualization | Boot compatibility and device-specific presets |
| Display/input | Displays, scaling, touch/input availability | Readable and handheld-friendly controls |
| Device type | Evidence for handheld, laptop, desktop, or VM | Explainable, editable edition recommendations |

Use sysfs/udev and structured native APIs from Rust. Keep routine discovery unprivileged. External probes have timeouts and structured parsing where possible. Scanning never mounts unknown filesystems, flashes firmware, installs host drivers, or changes disks.

Unknown and unsupported are distinct. An architecture string alone does not prove x86-64-v3 compatibility: check the catalog's exact CPU requirements. Select graphics branches by supported device IDs and upstream policy, rather than vendor name alone. Keep both GPUs on hybrid laptops. Recommendations must never select or approve an erase disk.

The compact hardware strip shows CPU, GPU, RAM, firmware mode, and storage readiness. A details drawer shows evidence, driver/firmware suggestions, confidence, and blockers. It stays usable while other scan tasks complete.

## Integrated Wi-Fi in TUI and GUI

Share a Rust `borea-network` client using NetworkManager D-Bus through zbus. The installation environment supplies the service, system bus, device driver, and network firmware. Borea Live always does; another live session may not, and `borea doctor` reports which is missing. The TUI can connect before downloading the GUI.

Support network/device selection, signal and security display, refresh, WPA2/WPA3 personal networks, password reveal, hidden networks, disconnect/reconnect, and explicit open-network selection. Enable a software-blocked radio only on user action; explain hardware rfkill switches.

Represent connecting, authentication rejection, association, address acquisition, captive portal, DNS/TLS failure, offline, and internet-ready separately. A Wi-Fi link alone does not establish access to release/package servers. Use bounded minimal connectivity checks with a documented endpoint, handle proxies, and avoid disruptive repeated scans.

Delegate authorization to the system policy agent and secrets to a scoped NetworkManager secret-agent flow. Passwords never appear in command arguments, plans, or logs. Treat SSIDs as untrusted text. For enterprise networks, preserve certificate validation; support reviewed EAP flows or open the supported system editor with a precise explanation.

Connections default to live-session-only using in-memory profiles where supported. **Reconnect after installation** is an explicit choice, enabled only when the adapter can transfer settings securely. Do not silently copy enterprise identities/certificates. Target profiles use restrictive permissions and never expose credentials in exported plans.

If NetworkManager or required firmware is absent, show the missing dependency and offer Ethernet, USB tethering, or supported live-environment tools. Borea Live includes a broad licensed network-firmware baseline, avoiding the problem of needing a large download before Wi-Fi works.

## Target drivers and firmware

Resolve detected device IDs against reviewed mappings for the chosen release. Install appropriate CPU microcode, GPU components, network/Bluetooth/device firmware, and controller support before finishing. Include 32-bit graphics/Vulkan libraries when the gaming stack needs them. Verify package signatures and show packages and repositories in review.

Image-based targets use native image capabilities or supported extensions; do not mutate their base with generic package operations. OS-loaded firmware is distinct from BIOS, SSD, controller, or other firmware flashing. Flashing is never a side effect of discovery or the gaming preset.

Validate package presence, boot-artifact inclusion, and target configuration. Runtime driver binding on the installed system is confirmed after boot; package installation alone does not prove hardware operation.

## Tumbleweed Gaming + Firmware

Enable this preset by default. Install before first boot:

- Steam and native Proton prerequisites; Steam-managed runtimes can download on first use.
- Lutris and a supported Wine stack.
- MangoHud, GameMode, and Gamescope where compatible with the graphics/session stack.
- Native 64-bit and required 32-bit graphics/Vulkan libraries matched to detected GPUs.
- CPU microcode and device/network/Bluetooth firmware packages.
- Supported controller permissions and handheld device integration.

Heroic and other launchers can be optional extras from reviewed sources. Resolve exact package names for the selected snapshot instead of embedding assumptions in the common engine. Explain non-OSS components and additional sources in review. Do not silently add Packman/third-party repositories or every possible GPU driver.

Preserve Tumbleweed's supported security, filesystem, and native update behavior. Its layout is not constrained to the first Arch ext4 slice. Certify Limine, initramfs regeneration, selected filesystem features, native upgrade, and reboot together.

## Acceptance cases

Cover AMD, Intel, NVIDIA, hybrid graphics, CPU ISA mismatch, handheld/VM identification, hotplug, partial scan errors, missing firmware, rfkill, wrong Wi-Fi passwords, hidden networks, reconnect, captive portals, and network loss during downloads. Exercise TUI Wi-Fi before GUI assets are cached. Verify secrets never enter diagnostics and hardware identifiers are minimized in support exports.

For Tumbleweed, inspect the installed package/source manifest, boot on representative graphics/network hardware, launch gaming tools, update using native tools, and reboot. Gaming-service accounts and game downloads remain user-controlled.
