# Arch and CachyOS + Noctalia v5

Choose **Arch or CachyOS → Minimal → Compose a desktop → Hyprland, niri or MangoWC → Noctalia v5**.

```text
Borea → disk → account and region → review → install
Reboot → graphical disk unlock → Noctalia Greeter → chosen compositor + Noctalia
                                                        └─ selected apps finish quietly
```

| Compositor | Graphical session | Screen sharing portal |
| --- | --- | --- |
| Hyprland | `start-hyprland`; Lua startup hook | Hyprland |
| niri | `niri-session`; `spawn-at-startup` | GNOME; Xwayland satellite for X11 apps |
| MangoWC | `mango` in a D-Bus session; `exec_once` | wlroots |

All three use signed upstream packages. CachyOS adds its signed repository,
`linux-cachyos`, settings and branding; Arch uses its stock kernel.
CachyOS's separate themed Noctalia presets remain previews.

- Erase-disk and alongside-Windows use the same reviewed storage engine.
- Linux storage uses LUKS2 and Btrfs. Alongside keeps Windows and its EFI files.
- Secure Boot is optional. Graphical login requires a password; no TTY setup or autologin.
- The desktop, drivers, audio, portals and greeter install before reboot.
- Selected extra packages and Flatpaks finish quietly after the account's first active Wayland login. Transactions are checkpointed and retried.

## Shortcuts and region

Shortcuts follow [Noctalia's v5 compositor guides](https://github.com/noctalia-dev/noctalia/tree/v5.4.1/docs/user/compositor-settings).
Window and workspace keys follow each compositor's upstream defaults.

| Action | Hyprland | niri | MangoWC |
| --- | --- | --- | --- |
| Launcher | Super+Space | Super+Space | Super+Space |
| Control Center | Super+S | Super+S | Super+S |
| Settings | Super+comma | Super+Shift+comma | Super+comma |
| Window switcher | Alt+Tab | Alt+Tab | Upstream MangoWC Alt+Tab |
| Audio / brightness | Noctalia media keys | Noctalia media keys | Noctalia media keys |

Noctalia keeps its internal navigation defaults and handles lock-before-suspend.
Borea writes your choices before creating the account:

| Choice | Applied to |
| --- | --- |
| Language | System locale and Noctalia UI; upstream translation fallback applies |
| Keyboard and variant | Greeter, compositor and lockscreen |
| Console keymap | Encrypted disk unlock and initramfs |
| Time zone | System, greeter and desktop |

## Release gate and testing

**31 October 2026:** upstream stable v5 is **5.4.1**; the signed Arch package is
**5.4.0-1**. Installation stops before disk changes until the selected repository
provides the approved version. Borea does not compile a replacement or silently
install an older release.

- Planner tests cover all six combinations, both storage layouts, selected packages, greeter, region and first-login app deferral.
- Iced tests carry each compositor choice into the serialized native plan.
- Real **Hyprland 0.57.0**, **niri 26.04** and **MangoWC 0.18.4** parsers accept the generated configs. CI repeats these checks using signed Arch packages.
- [Verification record](../backend/docs/verification/noctalia-compositors-2026-10-31.json).
- Full graphical login, lock/unlock, suspend/resume and physical Arc A770 testing remain outstanding. A MangoWC headless attempt stopped because its renderer requires a DRM device.

The disposable VM workflow accepts `desktop: noctalia`, `noctalia-niri` or
`noctalia-mango`, with Arch or CachyOS and either disk layout. Its serial checks
verify installed services and config files; they do not certify graphical login.

```sh
cargo run --locked -p borea-backend --bin borea-verify -- arch --check-noctalia
borea-firstboot --status
journalctl -u borea-firstboot.service
```
