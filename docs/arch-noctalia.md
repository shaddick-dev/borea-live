# Arch, CachyOS and Tumbleweed + Noctalia v5

Choose **Arch, CachyOS or openSUSE Tumbleweed → Minimal → Compose a desktop → Hyprland, niri or MangoWC → Noctalia v5**.

```text
Borea → disk → account and region → review → install
Reboot → graphical disk unlock → Noctalia Greeter → chosen compositor + Noctalia
                                                        └─ selected apps finish quietly
```

| Compositor | Graphical session | Screen sharing portal |
| --- | --- | --- |
| Hyprland | `start-hyprland`; Lua startup hook | Hyprland |
| niri | `niri-session`; `spawn-at-startup` | GNOME; Xwayland satellite for X11 apps |
| MangoWC | `mango` using the login’s user bus; `exec_once` | wlroots |

All three use signed upstream packages. CachyOS adds its signed repository,
`linux-cachyos`, settings and branding; Arch uses its stock kernel.
CachyOS's separate themed Noctalia presets remain previews.

### Tumbleweed

Tumbleweed installs the same recipes from its own packages: Noctalia, Noctalia
Greeter, Hyprland and niri come from the official OSS repository. MangoWC is not
packaged by openSUSE, so it comes from the community OBS project
[`home:darix:playground`](https://build.opensuse.org/project/show/home:darix:playground),
which builds the same 0.18.4 Arch ships. Like Packman, its signing key's SHA-256
is recorded at review and checked before import; the repository appears only
when MangoWC is chosen.

| | Arch / CachyOS | Tumbleweed |
| --- | --- | --- |
| Packages | Signed Arch (and CachyOS) repositories; AUR first for apps | OSS, non-OSS for Steam, Packman for codecs, OBS for MangoWC; Flathub for apps |
| Boot | systemd-boot, mkinitcpio | systemd-boot via `sdbootutil`, dracut |
| Extra apps | Quietly after first login | During installation (Flatpak after first login) |

Hyprland is 0.56.2 on Tumbleweed and 0.57 on Arch; both read the same
`hyprland.lua`, which CI checks against each.

- Erase-disk and alongside-Windows use the same reviewed storage engine.
- Linux storage uses LUKS2 and Btrfs. Alongside keeps Windows and its EFI files.
- Secure Boot is optional. Graphical login requires a password; no TTY setup or autologin.
- The desktop, drivers, audio, portals and greeter install before reboot.
- Selected extra apps finish quietly after the account's first active Wayland login: AUR first, official packages next, Flatpak as fallback. Transactions are checkpointed and retried.

## Shortcuts and region

Each compositor is set up as [Noctalia's v5 compositor guides](https://github.com/noctalia-dev/noctalia/tree/v5.4.1/docs/user/compositor-settings)
describe, all of it: autostart, rounded windows, gaps, shadows and blur, its
settings window floating, layer rules and shortcuts. The compositors' own example
configurations are not used. Noctalia defines no window keys, so all three share
one small set:

| Action | Hyprland | niri | MangoWC |
| --- | --- | --- | --- |
| Launcher | Super+Space | Super+Space | Super+Space |
| Control Center | Super+S | Super+S | Super+S |
| Settings | Super+comma | Super+Shift+comma | Super+comma |
| Window switcher | Alt+Tab | Alt+Tab | — (not in Noctalia's Mango guide) |
| Audio / brightness | Noctalia media keys | Noctalia media keys | Noctalia media keys |
| Terminal (foot) | Super+T | Super+T | Super+T |
| Close window | Super+Q | Super+Q | Super+Q |
| Workspace / move to it | Super+1–9 / Super+Shift+1–9 | Super+1–9 / Super+Shift+1–9 | Super+1–9 / Super+Shift+1–9 |
| Focus | Super+arrows | Super+arrows | Super+arrows |
| Float | Super+V | Super+V | Super+V |
| Log out | Super+Shift+E | Super+Shift+E | Super+Shift+E |

Noctalia keeps its internal navigation defaults and handles lock-before-suspend.
Borea writes your choices before creating the account:

| Choice | Applied to |
| --- | --- |
| Language | System locale and Noctalia UI; upstream translation fallback applies |
| Keyboard and variant | Greeter, compositor and lockscreen |
| Console keymap | Encrypted disk unlock and initramfs |
| Time zone | System, greeter and desktop |

## Release gate and testing

**31 October 2026:** Borea installs the newest stable Noctalia v5 package in the
selected distro's signed repositories, with **5.4.0** as the minimum supported
release. During verification, the signed repositories provided **5.4.1-1**.
A mirror still providing **5.4.0-1** remains supported; a newer GitHub patch
release no longer blocks native installation.

Review approves stable major v5. Native preparation checks the actual package
before target writes; prereleases, versions below 5.4.0 and other majors are
rejected. Package signatures remain required. No source compilation or web API
lookup is needed. Previously approved exact-version jobs retain their exact check.

- Planner tests cover all six combinations, both storage layouts, selected packages, greeter, region and first-login app deferral.
- Iced tests carry each compositor choice into the serialized native plan.
- Real **Hyprland 0.57.0**, **niri 26.04** and **MangoWC 0.18.4** parsers accept the generated configs. CI repeats these checks using signed Arch packages.
- All six profiles passed whole-drive, Secure Boot-off installation and boot in disposable VMs, with active greeter/account services and the chosen configs. [Installed-system results](../backend/docs/verification/noctalia-installed-2026-10-31.json).
- The native Noctalia 5.4.1 config validator also accepts the generated shell config. [Earlier package/parser record](../backend/docs/verification/noctalia-compositors-2026-10-31.json).
- Full graphical login, lock/unlock, suspend/resume and physical Arc A770 testing remain outstanding. A MangoWC headless attempt stopped because its renderer requires a DRM device.

The disposable VM workflow accepts `desktop: noctalia`, `noctalia-niri` or
`noctalia-mango`, with Arch, CachyOS or Tumbleweed and either disk layout. Its serial checks
verify installed services and config files; they do not certify graphical login.

```sh
cargo run --locked -p borea-backend --bin borea-verify -- arch --check-noctalia
borea-firstboot --status
journalctl -u borea-firstboot.service
```
