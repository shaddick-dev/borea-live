# TUI bootstrapper and release delivery

## Download and install without compilation

The product name is **Borea**. The normal user path downloads prebuilt artifacts;
compiling the live environment belongs to maintainers. Setup fails clearly if
the release is unavailable, without installing development tools. Source builds
require `--from-source` during setup or `--make`/`--build` in the USB helper.
After a failed ISO download, a checkout can explicitly compose an image with **b**
or `borea-rataTUI --compose`. This downloads prebuilt Linux components and builds
Borea's Rust apps, without a release or a kernel/Mesa source build. The ordinary
write screen does not expose whole-runtime source-build shortcuts.

The backend installs the chosen upstream OS, drivers, boot components and desktop
before rebooting. It records selected Flatpak apps in a root-owned first-boot queue
as part of the approved job. The installed Rust `borea-firstboot` service registers
Flathub and installs those apps noninteractively after networking starts. It uses
`Type=exec` so the desktop does not wait for all app downloads to finish. Successful
steps are checkpointed atomically, failures retry after 60 seconds, and progress
and errors are available through `journalctl -fu borea-firstboot.service`.
No arbitrary downloaded shell script is executed to install these apps.

Native apps and AppImages are still installed during OS installation. An explicitly
enabled AUR source can compile packages. Moving more optional apps to first boot,
visible desktop progress, and wiring GUI selections to the backend remain further
work. First-boot app installation requires connectivity on the installed system;
failed steps remain queued across restarts. An offline computer is not reported as
having completed app setup.

## Writing Borea Live to a USB stick

`borea-flash` is the supported way to make Borea Live media from Linux. `borea-flash borea-live-<version>.iso`, or no argument to find the newest `borea-live*.iso` in the current directory or `~/Downloads`. It:

1. Accepts only a bootable hybrid ISO 9660 image. When a `<image>.sha256` or `SHA256SUMS` file sits beside it, it checks the image against it and refuses to write an image that does not match.
2. Lists only removable and USB disks. The disk running this system, the disk the image is stored on (followed through LUKS and LVM), anything too small, swap, and in a live session any installer medium are shown with the reason they can't be used. Mounted sticks can be unmounted from the list.
3. Requires the device name (for example `sdb`) to be typed back, re-checks the stick just before opening it, and opens it exclusively. As root it opens `/dev/<name>` directly. Otherwise it asks UDisks2, which prompts through polkit, so `sudo` is not needed on a desktop.
4. Writes in 4 MiB chunks, syncs the device, drops cached pages, and reads every byte back to compare with the image. Stopping asks first and explains that the stick won't boot.
5. Offers to eject the stick when it is done.

There is deliberately no non-interactive mode that writes without the typed confirmation.

## Purpose

The `borea` terminal app is the entry point: it downloads the project as a verified, runnable installer bundle and launches the graphical installer. A source checkout is a separate optional developer path, not the default end-user download.

Proposed command interface; these commands are not implemented yet:

```text
borea                          # interactive bootstrap TUI
borea download                 # download and verify a compatible installer
borea launch                   # verify and launch an eligible cached installer
borea doctor                   # explain host compatibility and failures
borea --version
```

Channel and version selection can be added as explicit flags. A pinned historical version still requires authenticated metadata and a compatibility check; choosing a version must not silently bypass expiration or known-revocation policy.

## TUI screens

1. **Welcome:** detected machine, selected channel, compatible installer.
2. **Readiness:** network, architecture, host/runtime requirements, writable space, graphical session.
   Integrated Wi-Fi setup is available here before GUI assets are downloaded, through the shared NetworkManager client. Hardware scanning is local and read-only.
3. **Download:** artifact name, authenticated expected size, received bytes, transfer rate, retry state.
4. **Verify:** metadata trust, artifact hash, safe extraction, bundle compatibility.
5. **Launch:** open GUI, keep download, or show setup instructions.

The app restores terminal state on exit and common error paths. It handles terminal resize, keyboard-only interaction, cancellation, interrupted downloads, and clear nonzero exit codes. Terminal restoration after an uncatchable process kill cannot be guaranteed; provide recovery instructions where needed.

## Release trust and metadata

Recommended release model: TUF-compatible signed metadata with an embedded root of trust, separated offline root and online release roles, expiration, version counters, and documented key rotation/recovery. Final library selection requires a prototype and maintenance review.

Signed target metadata identifies:

- Release version, channel, build identity, and artifact digest/length.
- Architecture and platform/runtime requirements.
- Minimum bootstrapper version and IPC/plan schema compatibility.
- Adapter and profile catalog versions.
- Distribution-family/edition metadata, latest-release resolver identity, hardware constraints, metadata age, and integration status. Installer release version and target OS release version are independent.
- Limine version and bundle component identities.

HTTPS protects transport; signed metadata establishes release authorization. A checksum hosted beside an executable is not an independent authenticity mechanism.

Distribution packages use their own authenticated repository mechanisms. Borea must preserve those checks; signing the Borea bundle does not authenticate arbitrary packages fetched afterward.

Metadata expiration depends on a credible clock. Diagnose clearly when the live environment's clock is implausible; do not silently disable expiration. Cached/offline behavior must have a defined freshness policy. The online installer should not claim indefinite offline installation support.

## Download, cache, and launch

- Download into a private temporary file under the user's cache directory, with safe permissions.
- Enforce expected length, maximum size, timeouts, bounded retries, and cancellation.
- Use range requests only when the origin and artifact validator permit safe resumption. Verify the entire completed artifact regardless of resumed ranges.
- Verify target metadata and hash before extraction.
- Reject archive traversal, absolute paths, unexpected device nodes, unsafe links, and excessive expansion.
- Extract into a new versioned staging directory, validate expected files, and promote the complete bundle into the cache.
- Revalidate cached eligibility and content when launching; do not trust a prior "verified" text marker.
- Launch the GUI with a controlled environment and protocol version, preserving the user's display session.
- Keep cached versions separately and prune only inactive bundles. Disk-full errors must leave no half-published release.

Proposed Linux paths follow XDG conventions:

```text
$XDG_CACHE_HOME/borea/releases/<version>/<architecture>/
$XDG_STATE_HOME/borea/logs/
$XDG_CONFIG_HOME/borea/config.toml
```

Use normal XDG fallbacks when these variables are unset. Privileged worker state belongs in a separate protected location and is never trusted merely because it is under a user's cache directory.

## Bootstrapping trust

The downloader itself needs an authentic distribution path. Publish release binaries and detached signatures/checksums with documented verification, and later seek native distribution packaging. Users should not need an opaque remote script executed as root.

The project must choose a real release host, signing authorities, and domain before publishing working installation commands. This proposal intentionally supplies no fabricated download URL.

## Runtime compatibility

A small musl-linked TUI binary is a promising Linux bootstrap artifact, subject to testing. The GUI may need graphics/system libraries even when most Rust code is statically linked.

The release manifest must declare tested runtime requirements. Check them before download where possible and before launch in all cases. Requirements are expressed as host capabilities, never as a host distribution. Borea Live is the reference environment, a reproducible image with graphics, firmware, networking, disk tools, and the bootstrapper. Other live sessions are accepted when `borea doctor` finds the required capabilities. See [the vision](vision.md).

If a graphical session cannot be found, explain the requirement and retain the download. Do not silently start a compositor, modify the host's package set, or pretend that a TUI installation mode exists before it is implemented.

Windpad resolves current distribution releases only after connectivity is established. Cached catalog data displays its age; it does not automatically establish "latest." Pin exact target artifacts in the reviewed installation plan, and require fresh review if a resolver changes the plan. All requested editions can be visible while their adapters remain pending; only certified combinations may execute.

## Release process

1. Build from a reviewed tag with pinned dependency and toolchain inputs.
2. Run unit, download-verification, privilege-boundary, and VM installation gates.
3. Produce artifact inventory, software bill of materials, licenses, hashes, and provenance.
4. Sign and publish target artifacts and staged metadata in a consistency-preserving order, publishing the timestamp/freshness reference last.
5. Smoke-test downloads and installation through the public distribution path.
6. Promote through nightly, beta, and stable with separate eligibility policies.

Release signing keys must not be stored in the repository. Document rotation, compromised-key response, artifact revocation, and how a bootstrapper recovers from a revoked installer. A signed but untested build is not a supported release.
