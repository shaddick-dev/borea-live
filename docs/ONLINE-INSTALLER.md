# One independent online installer

Owner's direction, 25 October 2026: one distro-agnostic BOREA Live ISO; Rust
installation logic; online installation of Arch, Fedora, Debian and openSUSE;
shared account and disk setup comparable in workflow to Calamares.

## Channels

| Target | Selected channel | Native packages | Account administration |
| --- | --- | --- | --- |
| Arch | Rolling | pacman / pacstrap | wheel |
| Fedora | Rawhide, development | DNF / signed RPMs | wheel |
| Debian | Sid, unstable | APT / signed InRelease and package hashes | sudo |
| openSUSE | Tumbleweed | Zypper / signed RPMs and metadata | wheel |

Fedora stable and Debian stable are not rolling releases. Rawhide and Sid meet
the requested continuously changing channel model but carry development-channel
risk. Older catalog descriptions listing latest stable have not yet been
migrated; `borea targets` is the new four-target contract, not a claim that the
GUI already implements that contract.

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
