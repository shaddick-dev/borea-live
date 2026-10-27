# Rust architecture and trust boundaries

## Proposed workspace

This is the intended layout. `borea-core`, `borea-catalog`, `borea-host`, `borea-hardware`, `borea-distro`, and `borea-bootstrap` exist today as read-only foundations; the others are planned.

```text
borea/
  Cargo.toml
  crates/
    borea-bootstrap/     # `borea`: downloader TUI
    borea-gui/           # Windpad: the same interface for every target
    borea-core/          # domain types, errors, capability model
    borea-installd/      # privileged installation service (see backend/README.md)
    borea-protocol/      # its socket contract, shared by the GUI and borea-cli
    borea-ipc/           # versioned requests and events
    borea-storage/       # block topology, layout, mounts, filesystem operations
    borea-fetch/         # metadata, downloads, cache, authenticity checks
    borea-catalog/       # latest upstream releases, editions, eligibility
    borea-hardware/      # local progressive scan and hotplug facts
    borea-host/          # installation-environment kind and capabilities, never distro identity
    borea-network/       # shared TUI/GUI NetworkManager client and secrets
    borea-distro/        # adapter interface and shared configuration primitives
    borea-distro-arch/
    borea-distro-fedora/
    borea-distro-debian/
    borea-distro-cachyos/
    borea-distro-omarchy/
    borea-distro-bazzite/ # native image deployment/update/rollback
    borea-distro-nobara/
    borea-distro-pikaos/
    borea-distro-tumbleweed/
    borea-limine/        # versioned boot layout and configuration renderer
    borea-testkit/       # fake executor, fixtures, VM orchestration helpers
  profiles/             # desktop/package mappings, versioned with adapters
  live/                 # future reproducible installation-media build
  tests/vm/
  docs/
```

## Technology choices

| Area | Proposed choice | Reason and qualification |
| --- | --- | --- |
| GUI | egui/eframe | Rust-first, straightforward custom wizard and shared state; prove text input, accessibility, and live-environment rendering in a spike |
| Graphics | A tested eframe renderer configuration | Test Wayland, X11, and software-rendered VM environments; do not assume universal GPU support |
| TUI | Ratatui + Crossterm | Terminal layout, keyboard input, progress display |
| Hardware | Rust sysfs/udev access and bounded native queries | Progressive local read-only facts, hotplug invalidation |
| Wi-Fi | NetworkManager D-Bus via zbus | Shared TUI/GUI connections and scoped secret handling |
| Wind motion | egui painter paths and capped view-local animation | Reduced-motion/static mode, pause when inactive or reviewing disk changes |
| Async and HTTP | Tokio + Reqwest with Rustls | Streaming downloads, cancellation, TLS without assuming host OpenSSL |
| Serialization | Serde; TOML for authored profiles; JSON for IPC and exported plans | Explicit schemas and easy inspection |
| Domain errors | thiserror | Typed failures with user-facing recovery guidance |
| Logging | tracing | Structured, redacted stage and operation records |
| Release trust | TUF-compatible metadata, using a maintained Rust client after evaluation | Root rotation, target integrity, expiration, rollback protection |
| Privilege boundary | Unix-domain socket and a narrowly scoped worker | Separate UI/network interaction from privileged disk operations |
| Target configuration | Structured Rust code plus fixed native command arguments | Keep package manager and distribution conventions intact |

Pin compatible versions after the build spike. Commit Cargo.lock for application releases. Avoid prematurely promising a fully static GUI binary: graphics libraries and runtime requirements need an explicit compatibility contract even if the bootstrapper can be built with musl.

## Core objects

- `HostEnvironment` (implemented): environment kind (Borea Live, live session, installed system), capabilities, and informational OS identity. Eligibility never reads the identity.
- `HostFacts`: adds active mounts, block graph, and installation-media identification to `HostEnvironment`.
- `DistroCapabilities`: supported releases, host requirements, desktop profiles, filesystems, boot modes, encryption, and kernel/update integration.
- `InstallRequest`: user choices, with secret references rather than passwords.
- `InstallPlan`: resolved target versions, immutable disk identity and topology snapshot, operations, required artifacts, dependencies, and expected effects.
- `PlanDigest`: deterministic digest of the resolved plan, paired with a nonce and expiration for one installation attempt. It detects changes; it does not establish that the plan is safe by itself.
- `InstallEvent`: operation/stage identity, status, measured progress, user-visible message, and redacted diagnostic detail.
- `InstallJournal`: plan digest, operation outcomes, target identifiers, checkpoints, and validation results; no plaintext secrets.
- `HardwareSnapshot`: per-subsystem facts, confidence, timestamp, topology generation, and driver/firmware candidates; unknown is distinct from unsupported.
- `NetworkState`: adapters, radio state, access points, connection stage, and connectivity status; secrets live outside this object.
- `ResolvedRelease`: exact upstream release/build/snapshot, edition/image variant, hashes, metadata freshness, deployment strategy, and certification status.

The signed catalog populates family/distribution/edition dropdowns and upstream-specific latest-release resolvers. Latest selection is resolved and pinned before approval; catalog labels alone never authorize an install. See [distribution policy](distributions.md).

The engine has no UI dependencies. Both UI frontends can use the same discovery, planning, and event protocol. A later terminal installer therefore does not need a separate installation implementation.

## Planning and execution

```text
Discover -> Select -> Resolve -> Validate -> Review -> Approve
                                                       |
                                      Revalidate and authorize
                                                       |
                        Execute dependency-ordered operations
                                                       |
                                       Validate -> Finish
```

The planner rejects unsupported capability combinations before producing an executable plan. Planning is read-only with respect to target disks; network resolution and cache writes are permitted and identified as such.

Operations have typed inputs, preconditions, expected effects, retry rules, safe cancellation boundaries, and postconditions. Initial variants include fetching an artifact, creating a partition table, creating a filesystem, mounting a target, bootstrapping a base system, installing a package set, writing target configuration, building an initramfs, deploying boot files, and creating a UEFI entry.

The full catalog also requires typed native image-deployment operations, adapter-owned storage layouts, security labeling, hardware-specific driver/firmware resolution, and update/rollback validation. An image target must not be forced through the mutable-package operation chain. Networking is a separate user-authorized live-session service, available before any installation plan executes.

No adapter or profile can submit an arbitrary shell command over IPC. The worker maps supported typed operations to reviewed code and invokes necessary native programs using fixed executable resolution and argument arrays. Distribution package installation inherently executes package maintainer scripts as root; repository trust and controlled bootstrap environments remain part of the trust boundary.

## Privileged worker

The GUI and downloader run without root. A polkit-mediated launch is the preferred graphical path, subject to availability in the supported live environment. An explicit terminal elevation flow can be implemented for environments without polkit; password collection is delegated to the system's authentication mechanism.

The worker:

1. Uses a private IPC socket with restrictive permissions, peer-credential checks, protocol negotiation, a session nonce, and bounded message sizes.
2. Reconstructs or independently validates the plan, rather than trusting UI-supplied paths or a digest.
3. Binds execution to the authorized session and approved plan, with one active install lock.
4. Rechecks the target's identity, topology, usage, and mount state immediately before destructive operations.
5. Restricts writes to the selected disk, controlled target mounts, boot variables explicitly included in the plan, and private working directories.
6. Rejects host-path escapes and unsafe symlinks in target file operations; uses descriptor-relative and no-follow operations where appropriate.
7. Sends bounded, redacted progress and error events.
8. Exits when the operation finishes, or handles UI disconnection according to a defined safe-stop policy.

A graphical confirmation is a usability safeguard, not a security boundary against a compromised process running as the same desktop user. Root privilege acquisition and worker-side validation must stand on their own.

The worker must execute only verified release code. A later design should specify whether elevation performs an independent verification or uses a protected installed worker. An unprivileged, writable extracted worker cannot be treated as trusted merely because the downloader checked it earlier. The implementation gate requires an authenticated immutable handoff or protected staging with independent root-side verification, including protection against replacement between verification and execution.

## Bootstrap environment and target isolation

Host and target are independent. The host contributes only kernel and platform capabilities (`borea-host`). Each adapter brings its target's native tools from the target's own upstream sources: a signed bootstrap archive, a container image of the target, or an OS image, verified and run inside the private mount namespace. Even when host and target happen to match, as with Arch on an Arch live session, the adapter uses its upstream toolchain rather than the host's `PATH`. See [the vision](vision.md).

Use a private mount namespace, scoped target mount tree, controlled environment, explicit bind mounts, and deterministic cleanup. A chroot is a target setup mechanism, not a security sandbox. Bootstrap scripts and package repositories are trusted suppliers with a documented verification policy.

Secrets are short-lived typed values: do not serialize them into ordinary configuration, command arguments, traces, or support bundles. Transfer them over authenticated local IPC and use native tools via stdin or equivalent controlled channels. Redact error output and minimize retention; zeroizing an owned buffer alone does not prove all copies are erased.

## Download and extraction boundary

Fetch code validates signed metadata and artifact digests before exposing files to the launcher. Enforce download limits, archive size and expansion limits, safe paths, expected members, and link restrictions. Never extract directly into a privileged installation directory from the GUI.

The root worker must not trust an unprivileged cache pathname. Where it consumes cached inputs, it must use protected verified copies or stable descriptors plus independent content verification and safe parsing, with no time-of-check/time-of-use replacement opportunity.

## Dependency and supply-chain policy

Use reviewed crates, minimal enabled features, reproducible release inputs where feasible, license checks, dependency vulnerability checks, and artifact provenance. Distinguish Borea release metadata verification from distribution package signature verification; both are needed and use different trust roots.

Adapters initially ship with Borea and are reviewed as code. Downloaded community profiles are data constrained by a schema, not root-executed plugins. Third-party executable adapter loading is outside the initial design.
