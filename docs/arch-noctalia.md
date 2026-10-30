# Arch + Hyprland + Noctalia

```text
Borea Live → storage → account and region → review → install Arch
Restart → graphical disk unlock → Noctalia Greeter → Hyprland → Noctalia v5
                                                        └─ extra apps install quietly
```

| Before reboot | After the first graphical login |
| --- | --- |
| Arch, kernel, firmware and graphics drivers | Selected extra native packages |
| Hyprland, Noctalia v5 and Noctalia Greeter | Selected Flatpak applications |
| Networking, audio, portals and fonts | Printing service, when selected |
| Account, encrypted storage and bootloader | Checkpointed downloads and automatic retries |

- Choose **Arch Linux → Minimal → Compose a desktop → Hyprland → Noctalia**.
- Both erase-disk and alongside-Windows use the existing reviewed storage engine.
- Linux storage uses LUKS2 and Btrfs. Windows and its EFI files are preserved for alongside installs.
- Secure Boot is optional. The normal graphical login requires a password; no TTY login or autologin.
- The greeter launches `start-hyprland`; Hyprland's Lua startup hook starts Noctalia.
- Super+L opens Noctalia's own lockscreen. Super+Shift+S locks and suspends.
- Noctalia handles lock-before-suspend through logind's sleep-delay inhibitor.
- Packages come from Arch's signed repositories; this pathway does not build AUR packages.

## Release check

Borea resolves the latest stable **5.x**, records it in the approval digest and
checks the mirror's native package version before storage writes. It refuses an
older package, a prerelease or a different major version.

On 30 October 2026, upstream listed **5.4.1** while Arch offered **5.4.0-1**.
This is a release blocker, not a successful installation test.

```sh
cargo run --locked -p borea-backend --bin borea-verify -- arch --check-noctalia
```

## App progress

The root service waits for the chosen account's first active Wayland session.
Native transactions run sequentially and respect pacman's lock. Successful steps
are saved; failures retry after 60 seconds without opening a terminal.

```sh
borea-firstboot --status
journalctl -u borea-firstboot.service
```

## Verification

- All 36 required desktop/graphics/extra package names were found in the [official mirror database audit](../backend/docs/verification/arch-noctalia-2026-10-30.json).
- Automated tests cover both storage plans, approval/confirmation, graphical configuration, Arc A770 package selection and queue recovery.
- The VM workflow has a manual `desktop: noctalia` option with `layout: both`. Its boot check covers services and installed configuration, using serial access only for test instrumentation.
- A real graphical login, wrong-password rejection, lock/unlock, suspend/resume and Arc A770 hardware test remain required. The strict latest-version gate currently blocks full installation testing.
