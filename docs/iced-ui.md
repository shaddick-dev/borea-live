# Iced interface

Windpad and the live ISO run the same Rust/Iced 0.14 interface. Ratatui remains the USB helper.

- Nord dark and light colours. No purple accents.
- Official distro logos at 52px, without decorative frames.
- Right-to-left wind streaks and selected-card frost effects.
- Page changes ease the wind faster. Pause freezes every decorative effect.
- Hardware, network and install requests run on Rust workers, outside rendering.
- The ISO uses tiny-skia software rendering; an accelerated GPU is not required.
- Bundled Noto Sans keeps text independent of the host's font configuration.

```sh
cargo run --locked -p borea-gui --bin borea-live -- --window
cargo run --locked -p borea-gui --bin windpad
cargo run --locked -p borea-gui --bin borea-live -- --showcase /tmp/borea-shots
# Include the 24 fps frames used for the walkthrough video:
cargo run --locked -p borea-gui --bin borea-live -- --showcase /tmp/borea-shots --animate
```

Captures render the actual Iced widget tree with fixed example hardware. They do not contact the install service or write disks.

| Component | Responsibility |
| --- | --- |
| `desktop.rs` | Iced messages, views, Canvas effects and captures |
| `live.rs` | Explicit choices, wizard progress and installation eligibility |
| `install.rs` | Native storage/account/review worker |
| `handoff.rs` | Upstream Omarchy installation handoff |
| `wifi.rs`, `firmware.rs` | NetworkManager and firmware worker clients |
| `model.rs` | Catalog selection without GUI dependencies |

Tab/Shift-Tab move through text inputs. Number keys select visible choices; Ctrl+Left/Right return or continue outside the final install page. Final disk approval is explicit.

The migration removes Slint, egui and their rendering bridges. The backend remains Rust 1.85 compatible. Iced is also used by System76's COSMIC ecosystem; Borea does not depend on COSMIC.

[Logo sources](../crates/borea-gui/ui/logos/README.md) · [Design rules](design.md) · [Installation status](arch-noctalia.md)

The README GIF preview opens the MP4 player on GitHub Pages. The player keeps a static poster. Rebuild the video after
changing installer screens:

```sh
ffmpeg -framerate 24 -i /tmp/borea-shots/motion/frame-%04d.png \
  -vf "pad=ceil(iw/2)*2:ceil(ih/2)*2" -c:v libx264 -crf 20 \
  -pix_fmt yuv420p -movflags +faststart -an docs/assets/installer-showcase.mp4
cp /tmp/borea-shots/family-dark.png docs/assets/installer-showcase.png
```
