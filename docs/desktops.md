# Compose your desktop

Some editions ship without a desktop environment or shell: the **Minimal** edition of Arch Linux, CachyOS (upstream calls it "No Desktop"), Fedora Linux, Debian, and openSUSE Tumbleweed. On those editions, Windpad offers one clear path:

```text
Edition: Minimal  →  ① Compositor  →  ② Shell  →  review
```

Every other edition keeps its upstream desktop untouched. CachyOS KDE Plasma and KDE Plasma · Handheld, Omarchy, Bazzite, Nobara, and PikaOS ship their own desktop or shell, and layering a second shell over it invites conflicts, so the compose path never appears there. The catalog loader enforces this: a shell or compositor offered on a distribution without a `composable` edition is rejected (`CatalogError::NotComposable`).

## The path

1. **Choose a compositor.** Windpad shows Hyprland, niri, and MangoWC as three tiles. A compositor the distribution does not package stays visible but disabled, with "Not packaged for Debian" rather than vanishing.
2. **Choose a shell.** Only shells that support that compositor *and* install on that distribution appear. The suggested shell comes first. That is a setup made for this distribution, such as Mangobutter on Debian, if there is one. Otherwise it is the first shell installable from official packages, then the first offer.
3. **Review.** The installation review names the compositor, the shell, and exactly how each arrives.

The defaults are pre-filled, so a user who just presses Continue gets a sound desktop. Changing either step is one click.

**Every step is a complete answer.** The user can continue the installation at any point:

| Choice | Installs | Where in Windpad |
| --- | --- | --- |
| No desktop | Minimal exactly as shipped, with a console login | "No desktop" on the card, or "Skip · no desktop" in the compose dialog |
| Compositor, no shell | The compositor with its default config | "No shell · just Hyprland", the last card in step ② |
| Compositor + shell | Both, sourced as shown below | Any shell card |

In code this is `DesktopChoice::{None, Compositor, Shell}`. `Desktops::resolve` validates a choice against the distribution and returns how each part arrives; `None` is always valid. In the terminal, the same path reads `borea shells` → `borea shells arch` → `borea shells arch niri`.

## How shells arrive

| Source | What happens | Disclosure |
| --- | --- | --- |
| Official | Distribution package, installed with the system | Package names in review |
| Third-party repo | Named external repository (COPR, Terra, vendor APT repo) added for that package | Repository named in review before approval |
| AUR | Built **as the user** at first login, never as root | Shown in review |
| First-login setup | The project's own installer, started **as the user** at first login and pinned to a reviewed revision. Setups that install packages, like Mangobutter's `apt` step, ask for the user's password themselves | Installer URL shown in review |

This keeps the privileged worker's surface narrow. No upstream shell script runs as root during installation.

## Catalog (reviewed 2026-10-22)

Data lives in `crates/borea-catalog/data/shells.toml`. `borea-build concept-data` regenerates the Windpad concept's copy from the catalog itself, so the page and the Rust catalog never drift.

### Compositors

| Compositor | Arch | Fedora | Debian | Tumbleweed |
| --- | --- | --- | --- | --- |
| [Hyprland](https://github.com/hyprwm/Hyprland) | official `hyprland` | COPR `solopasha/hyprland`; Fedora retired its own package after F42 | official, `trixie-backports` | official |
| [niri](https://github.com/niri-wm/niri) | official | official | not packaged | official |
| [MangoWC](https://github.com/mangowm/mango) | official `mangowm` | Terra `mangowm` | butterrepo `mangowc` 0.14 (last series for trixie) | not packaged |

### Shells

Listed in order of GitHub stars on the review date.

| Shell | Built with | Compositors | Arch | Fedora | Debian | Tumbleweed |
| --- | --- | --- | --- | --- | --- | --- |
| [illogical-impulse](https://github.com/end-4/dots-hyprland) (end-4) | Quickshell | Hyprland | first-login | — | — | — |
| [Caelestia](https://github.com/caelestia-dots/shell) | Quickshell | Hyprland | AUR `caelestia-shell` | — | — | — |
| [Noctalia](https://github.com/noctalia-dev/noctalia) | native C++ (v5; v4 was Quickshell) | Hyprland, niri, Mango | official | official | `pkg.noctalia.dev` | official |
| [Dux Shell](https://github.com/duxlabs-dev/dux-shell) (formerly DankMaterialShell) | Quickshell + Go | Hyprland, niri, Mango | official `dms-shell` | first-login | first-login | first-login |
| [Serpantinum](https://github.com/ilyamiro/serpantinum) | Quickshell | Hyprland, niri, Mango | first-login | first-login | — | — |
| [Ambxst](https://github.com/Axenide/Ambxst) | Quickshell | Hyprland, niri | first-login | first-login | — | — |
| [iNiR](https://github.com/snowarch/iNiR) | Quickshell | niri | first-login | first-login | — | — |
| [ashell](https://github.com/MalpenZibo/ashell) | Rust (iced) | Hyprland, niri, Mango | AUR | — | — | official |
| [Mangobutter](https://justaguy.dev/drew/mangobutter) (JustAGuy Linux), made for Debian 13 | Quickshell | Mango | — | — | first-login | — |
| Waybar essentials (Borea preset) | Waybar · mako · fuzzel | Hyprland, niri, Mango | official | official | official | official |

## Extras

Every card's **Your desktop** panel ends with **Extras**: one checkbox per optional feature, on every edition, whether or not it has a desktop. The same extras appear on every distribution that provides them, each with its own packages, defined once in `crates/borea-catalog/data/extras.toml`:

| Extra | What it adds |
| --- | --- |
| Gaming | Steam, Lutris, GameMode, MangoHud, gamescope (CachyOS: its own gaming meta packages) |
| Media codecs | FFmpeg and the GStreamer plugin sets |
| Flatpak | Flatpak with the Flathub remote |
| Printing | CUPS and a printer settings tool |

Each checkbox says what it adds and any repository that needs ("enables RPM Fusion nonfree, for Steam"), and the review names it again. Everything starts unticked except Gaming on openSUSE Tumbleweed, which keeps its Gaming + Firmware default. What a distribution already ships in every edition is listed as "✓ Built into Bazzite: Gaming, Media codecs, Flatpak" instead of offering a checkbox. Adding an extra is one `[[extra]]` entry; the loader refuses entries for unknown distributions, built-in entries with packages, and entries with nothing to install.

### CachyOS Minimal: a gaming-first base

CachyOS's installer offers **No Desktop**, its optimized base with no desktop or shell; Windpad calls it **Minimal**, like every other desktop-free edition. Its Gaming extra uses CachyOS's own meta packages from its repository (checked 2026-10-22):

- `cachyos-gaming-meta`: Proton-CachyOS, Wine-CachyOS, umu-launcher, protontricks, winetricks, and the 32-bit libraries games need.
- `cachyos-gaming-applications`: Steam, Lutris, Heroic, Faugus, MangoHud, GOverlay, and gamescope.

CachyOS builds on Arch's repositories and ships `paru`, so every Arch compositor and shell source applies there too. Ahead of those, Windpad suggests CachyOS's own tuned desktop settings, marked "Made for CachyOS":

| Compositor | CachyOS settings (official cachyos repo) |
| --- | --- |
| Hyprland | `cachyos-hypr-noctalia` (Noctalia, noctalia-greeter) |
| niri | `cachyos-niri-noctalia` |
| MangoWC | `cachyos-mango-noctalia`, or `cachyos-mangowc-dms` (Dux Shell with the cat-dms theme) |

The same repository also has `gamescope-session-cachyos`, a Steam Gaming Mode session. It is not offered yet: it is a gamescope session rather than a shell on these compositors, and would be a separate opt-in alongside the desktop.

### JustAGuy Linux's butter projects (Debian)

[JustAGuy Linux](https://justaguy.dev/drew) maintains curated Debian 13 setups: Butterbian, the butterknife TUI installer, butterscripts, and per-window-manager setups. Two of them fill gaps in Debian's own archive:

- **[butterrepo](https://justaguy.dev/butterrepo)** is a signed community APT repository (`apt.justaguy.dev`, `butterrepo-keyring`). It packages `mangowc` with the wlroots 0.19 and scenefx libraries it needs, installed beside Debian's own, plus `quickshell`, `noctalia`, `matugen`, `awww`, and `xwayland-satellite`. This is how MangoWC reaches Debian, disclosed as a third-party repository.
- **[Mangobutter](https://justaguy.dev/drew/mangobutter)** (formerly mangowc-setup) is a complete MangoWC desktop for Debian 13 only. It adds a Quickshell bar, rofi, dunst, kitty, and theming that follows the wallpaper. Because it was made for Debian, Windpad suggests it first there.

Also from the same collection, but outside the Hyprland, niri, and MangoWC scope: `sway-setup` and `swayfx-setup` for Wayland; `dwm`, `oxwm`, `i3`, `bspwm`, `openbox`, `awesomewm`, and `qtile` setups for X11; and `cosmic-trixie`. Adding Sway as a fourth compositor would bring `sway-setup` in naturally.

Codeberg hosts mirrors only. The canonical repositories are on `justaguy.dev`.

### Considered and left out

- **HyprPanel**: its README announces it is soon to be archived, with the author building Wayle as a successor. Revisit Wayle when it ships.
- **Ironbar**: only partial niri support and no MangoWC support. A bar rather than a shell.
- **caelestia-kde, NextKde**: Plasma-based, so they belong with editions that ship Plasma.
- **Dotfile collections** (JaKooLit, personal rices): whole-system configurations rather than installable shells.

## Before certification

- Confirm that each first-login installer can be pinned to a commit and run non-interactively as the user.
- Confirm Waybar's MangoWC workspace module on each distribution's packaged version.
- Fedora 44 ships niri 25.11 while upstream is at 26.04. Shells that need newer niri IPC must be checked against the packaged version.
- Recheck this table each catalog review. The shell landscape moves quickly: DankMaterialShell became Dux Shell, and Noctalia left Quickshell, both within the last year.
