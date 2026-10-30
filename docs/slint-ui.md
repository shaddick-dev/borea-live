# Borea interface

Windpad and the live ISO installer now start the same Slint interface. The USB creator remains a Ratatui app with matching colours and quieter panels.

- Material 3 inspired surfaces, rounded controls and a restrained character-selection layout.
- Official distro logo images embedded in the application; no font-glyph logo substitutes or geometric frames.
- Light and dark themes, keyboard controls, accessible names and visible selection.
- Existing Rust selection model, NetworkManager worker, firmware restart and installer service clients.
- Native Arch + Hyprland + Noctalia and Omarchy handoff keep their existing approval and explicit disk-confirmation requirements. Other targets retain their existing preview limitations.
- Language, keyboard layout/variant, console keymap and timezone are separate explicit choices.

```sh
cargo run --locked -p borea-gui --bin windpad
cargo run --locked -p borea-gui --bin borea-live -- --window
cargo run --locked -p borea-gui --bin borea-live -- --preview --window
cargo run --locked -p borea-gui --bin borea-live -- --showcase /tmp/borea-showcase
```

`--showcase` uses Slint's software renderer, fixed example hardware and a simulated network. It does not contact the installation service or modify disks. Its PNG captures generate the README GIF.

Slint 1.11 is pinned for compatibility with the backend's existing D-Bus dependency and Rust 1.85 support. The GUI binaries render with Slint; the older egui view and its regression tests are retained alongside the existing state/service implementations. This avoids changing installation logic during the rendering migration.

Artwork sources: [bundled logos](../crates/borea-gui/ui/logos/README.md). Design references: [Material 3](https://m3.material.io/) and [Slint](https://docs.slint.dev/latest/docs/slint/).

## Verification

The Slint software renderer captured all wizard pages in both themes, the 900×600 layout, and native storage/account screens. Native startup passed under cage in a Debian 13 runtime with no GPU access (`BOREA_GUI_READY`). GUI, terminal and composer tests, Clippy, formatting and the backend Rust 1.85 check passed. A newly composed ISO boot and physical hardware install were not run for this redesign.

The GUI package uses Slint under its GPLv3 licence option. Artwork retains the upstream projects’ rights.
