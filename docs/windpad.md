# Windpad and wind design

## Layout and hierarchy

Create a calm native Rust interface using a Nord palette (snow-storm light or polar-night dark), fine frost-colored borders, generous whitespace, and a muted frost-blue accent. It is set entirely in JetBrains Mono Nerd Font, regular and bold, with Nerd Font icons doing the work of marks and pictures: distribution logos on the family and distribution choices, hardware icons on the machine facts, an icon per extra. Corners are tight, and there is no serif. Quiet airflow paths animate behind the hero.

Borea Live's separate Family page brings Claude's prototype hero-select treatment into the native installer: tall portrait cards, prominent distro emblems inside diamond frames, individual family accents, release labels, and an Explore family action. Distribution names come from the existing catalog. No family is preselected; selecting a card opens its distribution page. Cards reflow on narrower windows, retain keyboard focus and selection markers, and stop their hover lift when motion is paused. The Windpad landing-page layout below remains separate.

```text
 BOREA                                  Hardware   Wi-Fi   Appearance

                  Go wherever
                  the wind takes you.
      Choose your next Linux system. We'll prepare the way.

 [CPU]            [GPU]             [RAM]            [UEFI]

 Arch family                       Fedora family
 [ Distribution              v ]   [ Distribution              v ]
 [ Edition                   v ]   [ Edition                   v ]
 Latest rolling                    Latest stable / own channel

 Debian family                     openSUSE
 [ Distribution              v ]   [ openSUSE Tumbleweed        v ]
 [ Edition                   v ]   [ KDE Plasma                 v ]
 [ Your desktop · ☐ Extras     ]   [ Your desktop · ☑ Gaming …    ]

```

Menus read the same on every card: distributions by their upstream product name, editions by the desktop you get with upstream's own label after " · " ("GNOME · Workstation", "KDE Plasma · Handheld"), and the desktop-free edition always **Minimal**. A catalog test holds every entry to this.

Below the dropdowns, every card has a **Your desktop** panel. On Minimal it holds the compose path; otherwise it says which desktop comes with the edition. It ends with **Extras**, a checkbox per optional feature such as Gaming or Media codecs (see [desktops.md](desktops.md#extras)).

Composing a desktop lives on Minimal, and most people choose a desktop edition first, so every desktop edition's Your desktop panel offers the way there: **Or compose your own desktop on Fedora Linux Minimal**, with the compositors and how many shells it has ("Hyprland, niri, MangoWC · 6 shells, such as Noctalia, Dux Shell, and Serpantinum"). Where the distribution has no Minimal (Omarchy, Bazzite, Nobara, PikaOS), it offers the family's distribution that does. Borea Live's Desktop step makes the same offer.

The family cards flow with the window: four side by side when each gets at least 380 px, else two, else one. Each row is as tall as its tallest card, measured as it draws, so the cards' feet line up and the row grows or shrinks as choices change.

Each card's path ends in a preview directly beneath its controls: **Your selection**, the chosen distribution, edition, and desktop, and one **Continue with this selection →** action. The preview appears only once the path is complete. An edition with its own desktop completes as soon as its card is chosen. A Minimal edition completes on **No desktop**, or when a compositor and shell are confirmed with **Use this desktop**. Changing any choice withdraws the preview until the path is complete again, and only the selected card shows one. Dropdowns affect only their card; edition/driver options follow upstream availability and hardware compatibility. Keep derivative identity visible. Advanced controls expose exact release identity, graphics branch, packages, sources, boot path, and certification status.

Show **Checking latest release…**, then the exact resolved identity and check time. **Offline**, **Integration pending**, and **Unsupported on this hardware** are different states. Menus can show pending requested editions while installation remains disabled.

Hardware and Wi-Fi open side drawers without losing selections. Keep both available through the wizard. Storage and destructive approval are later explicit steps.

## Motion

| Element | Behavior | Proposed timing |
| --- | --- | --- |
| Hero background | Three to six fine curved streamlines with gentle drift/dash phase | 18–30 second loops; low opacity; no flashing |
| Navigation | Short directional translation and fade | 160–220 ms ease-out |
| Selected card | Border/accent transition; no moving controls | 120–160 ms |
| Download | Determinate bar driven by real bytes | Smooth rendering, accurate values |
| Scan | Small activity mark while results are pending | Stops when complete |

Use egui painter paths with view-local animation time. Cap repaints while active, stop decorative motion when inactive/obscured where practical, and pause it during critical review/errors. A static screen must not consume an uncapped render loop.

Honor platform reduced-motion preferences where available and provide a persistent in-app control. Reduced motion uses static airflow and no translations. Paths never cross text, move layout, steal input, hide focus, or communicate essential state. Low-performance graphics can use the static version.

## Input and accessibility

Give cards/dropdowns stable accessible names and readable full edition labels. Support keyboard navigation, arrows, Enter/Escape, visible focus, high contrast, scaling, and handheld-sized touch targets. Drawers trap focus while open and return it to the initiating button. Verify the native backend's actual screen-reader behavior before claiming support.

Explain recommendations in text and label destructive actions plainly. Do not rely on animation or color alone.

## Native implementation

`crates/borea-gui` builds the `windpad` binary with egui/eframe 0.37 (glow renderer, Wayland and X11). It follows this document and the HTML concept closely, and takes everything from real sources:

- **Catalog:** families, distributions, editions, compositors, and shells come from `borea-catalog`, the same data as `borea shells`. Nothing is duplicated in the UI.
- **Hardware:** a background thread runs the `borea-hardware` scan. The strip shows "Scanning…" until it finishes, and the Hardware drawer lists every fact, including disk usage. No disk is ever preselected.
- **Wi-Fi:** a worker thread owns the NetworkManager connection, so the window never waits on D-Bus. The drawer shows connectivity, radio state, networks, a passphrase field (hidden by default), hidden-network joining, and Rescan.
- **Selection:** `model.rs` is pure state with the rules used by the HTML concept. Windows render from `&self` and record `Action`s, and `Windpad::apply` is the only place state changes.
- **Wind:** the concept's four streamlines with gusts travelling along them. It repaints at about 30 fps only while moving, and holds still when paused or when the window loses focus.
- **Fonts:** JetBrains Mono Nerd Font (Nerd Fonts v3.5.1, SIL OFL 1.1), built into the binaries from `crates/borea-gui/fonts/`, so Windpad looks the same on any live session. Noto Sans or DejaVu follow for scripts it does not cover. Icons are named by meaning in `src/icon.rs`; a test checks the font has every one, and buttons are announced by their words alone.
- **Accessibility:** every control has an accessible name, and the kittest tests fail if one is missing. Tests drive the window by those names, as a screen reader would.

Run it with `cargo run -p borea-gui`. `cargo test -p borea-gui` exercises every path. Set `WINDPAD_SHOTS=<dir>` to also render light and dark screenshots.

## Borea Live installer

The live ISO does not boot into Windpad's landing page. It boots into `borea-live`, which keeps Windpad's workflow, concepts, palette, wind, and card-style choices, but presents them as guided, full-screen steps:

| Step | What happens |
| --- | --- |
| 1 · Connect | The shared Wi-Fi panel. Wired connections need nothing. Offline is allowed but explained. |
| 2 · Machine | One card per fact: processor, graphics, memory, firmware, device, displays, each disk with its usage (the live USB shows as installation medium), network. Then "What Borea will do": microcode, hybrid graphics. |
| 3 · System | Family, distribution, and edition as rows of cards instead of dropdowns, plus image variant where there is one. |
| 4 · Desktop | For editions without a desktop: Compose a desktop or No desktop, then compositor tiles and shell cards, including No shell. Editions with their own desktop just say so. Every edition ends with the Extras checkboxes. Leaving the step confirms the choice. |
| 5 · Review | A notice that this is a preview build that cannot install yet, then the full selection and a note that storage and accounts come next. **Begin installation** stays disabled, marked "Not in this preview build". The rail also carries a *Preview build · cannot install yet* tag on every step. |

The rail on the left shows every step: ✓ done, ● current, ○ ahead. Each completed step shows what was chosen ("CachyOS · Minimal", "MangoWC + Noctalia · CachyOS"). You can jump back to any step you have reached. Changing the family, distribution, or edition sends you back through Desktop before Review. Status, theme, and the wind toggle sit at the bottom of the rail. Wind flows across the header of every step and through the rail's mark. Enter moves on and Escape goes back whenever no control has focus.

Nothing has to be started or pressed by hand. On Borea Live it starts with the system (see `live/README.md`). The hardware scan begins at once. It waits for NetworkManager if the network comes up after it, rescans Wi-Fi on start, and refreshes every few seconds. If the machine is already online it moves past Connect by itself. If not, the strongest network is already chosen with the cursor in its passphrase field, Enter joins it, and the installer moves on once online.

It starts full-screen; `--window` runs it in a window for development. `tests/live.rs` walks every path, including the rail and the keyboard.

## Interactive concept

[Open the Windpad concept](../design/windpad.html).

The standalone HTML reference previews dropdowns, drawers, themes, and wind animation. It uses sample hardware and simulated networking, performs no real scan/connection/release lookup/install, and does not change the Rust-native architecture. Static edition labels are research examples; the actual app uses the signed catalog.
