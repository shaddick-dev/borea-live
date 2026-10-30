# One independent online installer

Owner's direction, 25 October 2026: one distro-agnostic BOREA Live ISO; Rust
installation logic; online installation of Arch, Fedora, Debian and openSUSE;
shared account and disk setup comparable in workflow to Calamares.

## Upstream distribution selection

The live wizard now separates **Family** from **Distribution**. Selecting a family
opens the next page; distribution, edition and any image variant require explicit
choices. There is no "None" distribution. Changing family clears its dependent
choices; changing distribution or edition invalidates later review. Minimal
editions also require an explicit desktop mode, compositor and shell decision.
"No desktop" and "No shell" remain valid explicit desktop choices.

| Family | Distribution choices |
| --- | --- |
| Arch | Arch Linux, CachyOS, Omarchy |
| Fedora | Fedora Linux, Bazzite, Nobara |
| Debian | Debian, The Butterbian Project, PikaOS |
| openSUSE | openSUSE Tumbleweed |

Butterbian's requested Xfce and mangobutter editions are visible for preview.
Its official URL, release model and edition artifacts have **not** been verified:
there is no fabricated download URL, inherited Debian package recipe, or claimed
working adapter. The catalog represents an absent source explicitly and refuses
to certify it. Its acquisition specification is `PendingVerification`.

Tumbleweed has Gaming, Media codecs, Flatpak and Printing extras on the next page.
All optional extras start unticked. Their package names and repository needs are
kept in `crates/borea-catalog/data/extras.toml`; executable installation still
requires release-specific resolution and verification.

BOREA is the aggregator, customizer and installer, not a new distribution. Download
upstream's published packages, root filesystem or native OS image, verify it, and
apply only selected changes. Keep the derivative's repositories, identity, kernel,
boot requirements and update mechanism. Bazzite needs its native image deployment
and rollback model. An Arch base with an arbitrary set of packages must not be
presented as a verified CachyOS or Omarchy installation. Adapter specifications
are research contracts until tested against upstream's actual artifacts.

## Channels

These are offered channels, not preselected choices. The user must pick the
distribution and its channel explicitly.

| Target | Offered channel | Native packages | Account administration |
| --- | --- | --- | --- |
| Arch | Rolling | pacman / pacstrap | wheel |
| Fedora | Rawhide, development | DNF / signed RPMs | wheel |
| Debian | Sid, unstable | APT / signed InRelease and package hashes | sudo |
| openSUSE | Tumbleweed | Zypper / signed RPMs and metadata | wheel |

Fedora stable and Debian stable are not rolling releases. Rawhide and Sid are
development channels, offered by the earlier four-target CLI foundation.
The derivative catalog preserves each project's upstream channel instead of
forcing it onto Rawhide or Sid. `borea targets` still describes only those four
base profiles; GUI derivative choices are not yet mapped to executable plans.
A future release selector must resolve an explicitly chosen, supported upstream
channel before acquisition. Do not label stable derivative releases as rolling.

## No implicit installation selections

Owner's clarification: nothing is default; every installation choice is picked.
The new installation flow must start with distribution, channel, edition,
desktop/compositor/shell, optional extras, disk/layout, account, locale/timezone,
keyboard and boot choices unset. Choosing "none" is an explicit decision.
Changing a parent choice must invalidate dependent choices instead of selecting
the first available replacement. Hardware detection supplies facts and hints,
not silently accepted answers. Navigation cannot complete a choice on the user's
behalf. Internal operational values such as log paths are not installation choices.

The CLI now requires explicit distro/channel/edition, disk, username, hostname,
timezone, locale, keyboard and extras decisions before it scans hardware or
downloads anything. It accepts either `--extras none` or specific `--extra ID`
options. The live preview now gates family/distribution/edition/variant and composed
desktop choices explicitly; its indexed card model still contains internal
placeholders, which must not be consumed directly as an installation plan.
The separate legacy Windpad card UI still has automatic selections. Storage,
account, layout and explicit boot choices remain required before live GUI
installation integration. The current GUI is still a non-installing preview.

## What this change implements

- `borea-engine::common`: shared read-only preflight for explicit disk choice,
  stable device identity, disk use and size, x86-64/UEFI, Secure Boot state, time
  zone and account/locale inputs. Arch now uses it instead of its own duplicate.
- `borea-engine::account`: Rust-generated native account arguments and sudoers
  content for all four targets. Debian uses sudo; the other profiles use wheel.
  Normal accounts get home directories and private user groups; root is locked.
  The Arch worker now consumes this shared logic. Passwords remain on stdin to
  chpasswd, never in a plan or command argument.
- `borea-engine::online`: target channel/source/verification contracts and a
  sequential progress tracker that rejects skipped or repeated stages. This
  tracker is a foundation; the existing worker does not yet drive it, and an
  ordered stage alone is not evidence its action succeeded.
- `borea targets`: read-only inspection of the contracts. Arch execution
  availability refers only to the existing Minimal CLI path, not all editions.
- `borea plan --distro`: explicit selection. Unknown, duplicate, misspelled or
  valueless options fail. Unsupported adapters fail before hardware scanning,
  downloads, or plan output instead of silently producing an Arch plan.

## Execution work still required

The added profiles are not operational Fedora, Debian or openSUSE installers.
They do not yet resolve a signed tool root, download a target package set, write
those systems or make them boot. No four-distro installation success is claimed.

1. Acquire each distribution's native tooling into a private verified tool root.
   Resolve an exact release/snapshot and digest from trusted signed metadata.
   Do this before erase approval; never install packages into BOREA's live root.
2. Produce a versioned common executable plan with pinned artifacts, package
   selection, target-specific configuration and boot/update strategy. Extend
   worker schema validation and the digest review together, not by relabeling
   the current Arch-specific plan.
3. Extract the worker's GPT, EFI/ext4 formatting, mount namespace, stable-device
   recheck, UUID fstab, logging and mount-aware cleanup into shared execution.
   Only the explicitly selected free whole disk is in the first implementation;
   encryption, resize, manual partitioning and dual boot need separate work.
4. Implement package deployment: Arch pacstrap; Fedora DNF installroot and
   SELinux setup; Debian native bootstrap plus APT Sid; openSUSE Zypper root
   deployment. Root-relative configuration and package scriptlets need native
   mount environments. Preserve signature/TLS verification at every stage.
5. Configure accounts, locale, keyboard, timezone, hostname, network services,
   firmware, kernel, initramfs and boot. Keep target kernel-update hooks working.
   Installing Limine alone does not establish Fedora/openSUSE boot compatibility.
6. Connect Windpad to storage/account forms, exact erase review, explicit
   confirmation, privileged worker launch and reported progress. Never enable
   the preview's install button merely because a target has a profile.
7. From the *same BOREA ISO*, install each target to a disposable virtual disk,
   remove the ISO, boot, log in as the new administrator, update kernel packages,
   reboot and log in again. Retain image hashes, logs and asserted results.

The ISO contains the live kernel, drivers/firmware, networking, UI and installer.
Target packages and tool roots are downloaded on demand; the ISO does not bundle
four distribution images. Rust owns BOREA logic and invokes native Linux tools
with argument arrays. Kernel/package-manager implementations and Buildroot's
configuration formats are external dependencies. ISO size and build-cache size
are separate measurements; no new ISO was built in this change.

## References inspected

- Calamares: the GitHub README confirms a distribution-independent installer
  framework with automated/manual partitioning and external modules. Development
  moved to Codeberg. The requested Codeberg URL returned HTTP 403 here, so the
  current Calamares online module implementation was not inspected or copied.
- netboot.xyz: its GitHub README describes iPXE network boot selection. The
  requested documentation URL returned HTTP 403. Network booting a target's
  installer is different from installing every target from one retained live
  BOREA environment; the latter remains the requirement.
- `morluto/rea`: its current README is "Reverse Engineer Anything", an MCP
  reverse-engineering toolkit, not a distro-agnostic Linux installer. No REA
  code or runtime was installed.

## Validation

Targeted checks: `cargo test --locked -p borea-engine -p borea-bootstrap -p
borea-worker`, formatting, Clippy, and read-only CLI smoke checks. No host disk
is erased during development. QEMU is not installed in this cloud workspace,
and the original Buildroot output/ISO is absent; new end-to-end installation and
boot results require preparing a VM/image-build environment. Unit tests do not
replace those results.

Results for this foundation: 20 tests passed (14 engine, two CLI, four worker).
Clippy with warnings denied and formatting checks passed. The `borea targets`
command and unsupported-target rejection were also checked as actual CLI
invocations. These results do not certify any new distro installation.

## Family/distribution page validation

The GUI integration tests cover automatic navigation after a family click,
mandatory distribution/edition/variant decisions, action/keyboard guards, changing
parents after review, explicit compositor/shell choices, Butterbian's unverified
state and opt-in openSUSE extras. Rendered screenshots are generated by setting
`WINDPAD_SHOTS` for the GUI tests. These validate the selection UI and catalog;
they do not validate deployment, rebuild the live ISO or prove an installed OS boots.

Results: 68 tests passed across the catalog, distribution adapters and GUI,
including 16 live-wizard integration tests and navigation at 900 × 600.
`cargo clippy --locked --workspace --all-targets -- -D warnings`, formatting and
whitespace checks passed. Light/dark and small-window screenshots were inspected.
