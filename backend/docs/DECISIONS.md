# Backend decisions

- Arch + Hyprland + Noctalia is a composed desktop installed from signed Arch
  binaries. greetd starts noctalia-greeter-session; password login starts
  Hyprland, which starts Noctalia v5. No TTY login or autologin is involved.
  Latest stable v5 is resolved at review, pinned in the approval digest and
  compared with the native package version before storage writes. A lagging
  Arch package blocks this pathway rather than building or substituting a source.
- The composed profile has no optional apps by default. Selected Arch extras
  live in borea-catalog. Native apps and Flatpaks queue in root-owned state and
  run after the first graphical login; mandatory desktop/driver packages install
  before reboot. Existing distro/profile behaviour stays compatible.
- Existing storage is UEFI + LUKS2 + Btrfs, whole disk or alongside Windows.
  This pathway uses those reviewed layouts and collects the disk/login passphrase
  before installing. Secure Boot remains an explicit GUI choice, initially off.

- The latest ZIP's backend contract governs this workspace: Arch rolling, Debian
  13 stable, Fedora stable (explicit release), Tumbleweed. It supersedes the older
  Rawhide/Sid base profiles for this backend. Derivatives remain a future adapter
  task; they cannot be simulated by renaming a base installation.
- Keep the uploaded catalog's apps/groups/drivers as the only runtime data source.
  The design directory retains an archival snapshot. Production changes happen here.
- Preserve the existing GUI workspace during integration; backend API types are
  owned/serializable and do not borrow the design catalog's static string lifetimes.
- Follow the plan's stated two-password semantics: the login passphrase unlocks
  LUKS; a separate admin passphrase sets root and sudo's rootpw policy. Never send
  either on argv or serialize it into a Plan or Job.
- Build scripts/generators and native Linux tools are dependencies; Rust owns
  planning, validation, protocol and execution orchestration.
- UEFI only, always encrypted. Package/tool-root verification and current disk
  facts must succeed before destructive execution. Existing media and mounted or
  unidentified devices fail closed. Never run installation against the cloud host.
- System profiles are explicit; partial step validation never supplies a distribution or later choices. A null desktop is accepted only when the client actually sends that key.
- Native repository signing keys for RPM Fusion/Packman are downloaded over the configured HTTPS upstream, hashed before Review and included in the approval digest. They are imported before signature-enforced native package operations; changing bytes invalidates approval.
- Native metadata/package existence checks occur in the RAM tool root before partitioning. Debian uses signature-verified debootstrap `--print-debs`; Arch resolves packages without downloading, Fedora queries package/group metadata, and openSUSE queries package information.
- Session CA trust is temporary operational configuration, passed into `/run` in chroots. The installer never disables certificate verification or persists the cloud session proxy trust into the installed OS.
- The service retains 4096 events with absolute cursors and offers both polling and streaming subscriptions. At most 16 client connections are handled concurrently.
- Native read-only diagnostic/probe commands run directly with fixed argv; all installation commands are executed by the privileged executor. Test harness shell scripts only construct disposable test environments and never form production installation commands.
- Plan v1 accepts an optional `secure_boot` target choice. Omission preserves the firmware's current policy; an explicit choice is included in approval. Preparing signed Arch boot with Secure Boot currently off requires firmware Setup Mode. Resetting firmware keys is never performed automatically. A requested unsigned install is refused while firmware Secure Boot remains enabled. This makes Arch's enrollment path reachable without pretending Setup Mode and active Secure Boot can coexist.
- Cleanup uses safe pidfds from `rustix` to stop only processes in the newly created, job-owned mount namespace. This extra dependency avoids PID reuse races when native key-management tools daemonize. Recursive bind mounts are unmounted from mountinfo, deepest first, with no lazy-unmount fallback; otherwise device mappings would remain busy after a successful install.
- Alongside installation keeps existing EFI/BOOT files in bounded RAM before native package/bootloader actions and restores them on success or failure before unmounting. Upstream bootctl `install` and sdbootutil can overwrite the fallback loader. Existing fallback binaries are never added to Arch's sbctl signing database. Synthetic NTFS/EFI preservation tests are distinct from real Windows boot certification.
- NTFS discovery never uses `ntfs-3g.probe --readwrite`: the upstream implementation opens O_RDWR and can adjust TXF metadata. Read-only `ntfsinfo --mft` checks volume flags and journal restart state; read-only ntfsls/ntfscat check a bounded hiberfil.sys header; `ntfsresize --info` supplies the minimum resize size. Unknown health blocks alongside installation. Nonzero/short hibernation headers are treated conservatively as active.
- Older ntfsinfo versions omit journal state. The fallback reads at most 256 MiB of $LogFile through read-only ntfscat and validates the erased journal or both version-1.1 restart pages (sizes, MST update sequences, client bounds and latest LSN). Unsupported/cached-metadata versions, oversized or torn journals fail closed. The native NTFS fixture hashes the entire image to check that discovery never changes its bytes.
- Login/disk/admin passphrases have at least eight characters and at most 512 UTF-8 bytes, matching cryptsetup's default interactive passphrase buffer. Stdin key-file input accepts longer keys, but allowing those could create an installation that cannot be unlocked at its boot prompt. Native MOK enrollment has the stricter 256-byte limit when required.

- Existing GPT entries are resized with native sfdisk -N, public size input on stdin, and signature wiping disabled. A native GPT-file test confirms that GUID, type, start, attributes and name survive. Parted script mode still prompts during shrinking.
- Debian native Steam uses contrib and i386. Native Steam/Lutris enable i386 before APT refresh; proprietary NVIDIA gaming adds the matching Debian i386 libraries. Debian auditing checks the actual component and architecture.
- Tumbleweed publishes dated networkd root archives and signed checksum metadata. Native repo URLs use HTTPS; temporary libzypp trust uses a hashed CA directory overlaid from RAM, restored before completion.
- Arch preparation resolves against an empty local package database with the signed sync indexes. Fedora finishing explicitly creates BLS entries and the signed-GRUB UUID redirect; its kernel command line precedes package scriptlets.

- NVIDIA’s sanctioned Tumbleweed repo also pins its upstream repomd signing key before Review, using the same digest/import path as Packman. Tool-root base refresh may import only keys referenced by the authenticated native root’s repo definitions. External repository keys are imported from their approved digests before refresh; missing key access blocks before disk writes.
- Tumbleweed preflight resolves the selected native set with Zypper install --dry-run before partitioning, so native capabilities such as versioned Python providers are accepted and dependency conflicts are caught. The catalog audit separately accepts exact native package providers and refuses header-only/no-match output.
- `bootctl --boot-path` selects XBOOTLDR and is supplied only when the layout has that separate partition. With a single ESP mounted at /boot, the ESP is sufficient.
- Native certificate-directory symlinks are canonicalized and must stay inside the owned authenticated root. Their actual mount path is recorded for restoration; native directory links are preserved.
- The synthetic post-resize NTFS check uses read-only ntfscat --force because ntfsresize deliberately schedules chkdsk. Production NTFS discovery/refusal rules are unchanged.
- DNF5 repoquery formats explicitly terminate package names with a newline; native multi-package preflight must not interpret concatenated names as missing packages. Nested Btrfs subvolume parents are created explicitly. Disposable boot automation accepts both cryptsetup and systemd passphrase prompt capitalization.
- Fedora classic GRUB uses an ext4 /boot, so kernel-install receives BOOT_ROOT=/boot through install.conf before RPM scriptlets and during final generation. The --boot-path option is reserved for XBOOTLDR and is not used there. Tumbleweed explicitly installs shadow for account management instead of assuming its minimal pattern provides useradd.
- Administrator groups are explicitly created idempotently before useradd. Tumbleweed uses its published systemd package and explicitly includes util-linux-systemd for fstrim.timer; the obsolete systemd-sysvinit name was rejected by the live audit. Minimal-pattern recommendations are not a dependency contract.
- Disposable serial-console matching strips terminal control sequences, including systemd OSC 3008 shell/session markers. Logs retain the original console, and success markers must still occupy a complete normalized line.
- Fedora files are labeled offline with the selected distribution’s setfiles and targeted policy, after final configuration. The -m flag allows labeling from a non-SELinux live kernel; device/proc/sys/run and the FAT ESP are excluded. Boot tests require SELinux enforcing rather than weakening policy for a successful boot.
- One Cargo workspace and lockfile for the whole repository. Backend crates moved to `crates/` and keep MSRV 1.85 and GPL-3.0-or-later in their own manifests; the live GUI crates need Rust 1.95. The GUI's own core and catalog crates were renamed `borea-live-core` and `borea-live-catalog` to end the name clash. zbus stays pinned at 5.5.0 (and zvariant at 5.9.2) because newer releases need Rust 1.87.
- `borea-installd` is the only installer. `borea-engine` and `borea-worker` (Arch Minimal, run by hand as root) were retired; Borea Live ships the service, `borea-cli` and the static first-boot helper. The socket contract (`borea-protocol`) is shared by the service, the GUI and `borea-cli`, so method names and payloads are checked at compile time.
- The live polkit rule grants udisks2 only the removable-media actions borea-flash uses. Writing internal disks is reserved to `borea-installd` and its approval digest.
