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
- Shortcuts follow [Noctalia's v5 Hyprland guide](https://github.com/noctalia-dev/noctalia/blob/v5.4.1/docs/user/compositor-settings/hyprland.mdx). Lock and suspend are available from Noctalia's session menu; Borea adds no custom lock/suspend shortcut.
- Noctalia handles lock-before-suspend through logind's sleep-delay inhibitor.
- Packages come from Arch's signed repositories; this pathway does not build AUR packages.

## Keys and region

| Key | Action |
| --- | --- |
| Super+Space | Noctalia launcher |
| Super+S | Control Center |
| Super+comma | Noctalia settings |
| Alt+Tab | Window switcher |
| Volume / brightness keys | Noctalia audio / brightness controls |
| Super+Q / Super+C | Terminal / close window (Hyprland defaults) |
| Super+arrows / Super+1–0 | Focus window / switch workspace |
| Super+Shift+1–0 | Move window to workspace |

Noctalia's internal navigation keys keep their upstream defaults. The old Borea
shortcuts (including Super+Return and Super+Shift+S) are removed from new installs.

Your Borea region choices apply independently:

- **Language:** generated system locale and Noctalia UI language. Noctalia falls back to English when a translation is unavailable.
- **Keyboard and variant:** Noctalia Greeter and Hyprland, including Noctalia's lockscreen.
- **Disk-unlock keyboard:** the selected console keymap, included in the initramfs.
- **Time zone:** the installed system's `/etc/localtime`; the greeter and desktop use it.

These settings are written before account creation. Existing installations and
personal keybindings are not overwritten by this installer change.

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

## Iced and CachyOS minimal

The Iced installer connects **Arch or CachyOS → Minimal → Hyprland → Noctalia** to the same Rust disk/account/review worker.

| Path | Source and behaviour |
| --- | --- |
| Arch | Signed Arch repositories and stock `linux` kernel |
| CachyOS minimal | Signed CachyOS generic x86_64 repository, inherited Arch packages, `linux-cachyos`, CachyOS settings and branding hooks |
| Omarchy | Verified upstream installation image and its own installer; Borea prepares the chosen whole-drive or Windows-alongside path |

- CachyOS is an explicit reviewed derivative, not an Arch installation renamed by Borea.
- Both native layouts use the selected account, language, keyboard, disk-unlock keymap and time zone.
- Required desktop and driver packages install before reboot. Optional apps finish after graphical login.
- Secure Boot off is supported. Owner-key signing requires firmware Setup Mode.
- Other compositor/shell combinations and CachyOS desktop presets remain previews; they are not certified by this integration.

**Upstream gate checked 31 October 2026:** latest stable Noctalia v5 is **5.4.1**; Arch's signed package is **5.4.0-1**. The native Noctalia path blocks before disk changes until the selected repository provides the approved version. Borea does not build an unofficial replacement or silently install an older release.

The planner tests cover CachyOS whole-drive/alongside layouts, kernel paths, required packages, optional app deferral and Secure Boot presets. These checks do not certify a full CachyOS graphical installation, lockscreen, suspend or physical Intel Arc A770 hardware.
