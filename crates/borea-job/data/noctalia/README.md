# Noctalia compositor templates

Each template follows Noctalia v5.4.1's guide for its compositor, all of it:
autostart, appearance, window and layer rules, blur and shortcuts
([guides](https://github.com/noctalia-dev/noctalia/tree/v5.4.1/docs/user/compositor-settings)).
The compositors' own example configurations are not used.

Noctalia defines no window-management keys, so Borea adds one small set shared
by Hyprland, niri and MangoWC: Super+T terminal, Super+Q close, Super+1–9 and
Super+Shift+1–9 workspaces, Super+arrows focus, Super+Shift+arrows move,
Super+V float, Super+Shift+E log out. Borea also applies the chosen keyboard
layout and variant.

niri uses Noctalia's stationary-wallpaper option, which matches Noctalia's
default of its backdrop section being off. Hyprland's template is in
`crates/borea-job/src/noctalia.rs`.
