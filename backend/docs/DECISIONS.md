# Backend decisions

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
