# Borea · app profiles design

Open `index.html` to try the revised page 05. If your browser restricts local files,
run `python3 -m http.server 8765` from this folder and open
http://localhost:8765. No Node dependencies or build step are needed to preview.
The browser preview opens at Apps; `?start=network` opens the original full flow.
This is an interactive design prototype. It does not install software or write disks.

## Page 05 flow

1. Nothing is preselected. Pick **Just the desktop** (zero extra apps) or
   **Everyday** (Firefox, LibreOffice, Okular, VLC, Printing, KeePassXC).
2. Optionally add **Gaming** (Steam, GameMode, MangoHud), **Creative** (GIMP,
   Kdenlive), or **Development** (git, VSCodium, Python). Packs can be combined.
3. Continue immediately, or open **Customize apps** to browse categories, search,
   add alternatives, or remove individual apps. Escape closes the editor.
4. The quiet summary shows the actual selected count, illustrative size, and
   additional repository requirements. Source details are available on demand.
5. Review uses the same selection. Returning to Apps preserves your changes.

The Everyday set is optional. A desktop-only choice is valid. This is separate
from choosing a distribution: there is no "None" distribution option.

## Selection behavior

The single app list is the union of the chosen starter and enabled packs, followed
by explicit per-app overrides. Duplicates are counted once. Overrides survive
starter and pack changes, so intentional edits are not silently erased.
**Restore profile apps** removes those overrides and restores the current starter
and packs. Removing a pack removes its suggested apps unless an explicit override
keeps an app. Extra sources are derived from actual app requirements; they stop
being required when the last dependent app is removed. Changing distro recomputes
these requirements. Source information and MB estimates are illustrative research
from the supplied prototype, not a verified package plan.

## Files

- `project/Prototype.dc.html`: updated editable Design Canvas component and state.
- `project/Packs.dc.html`: original Apps board importing the revised component.
- `project/canvas.json`: full original canvas, with the Apps board renamed.
- Other original boards and supplied logos are retained.
- `index.html`: generated standalone preview of the same component.
- `project/preview-runtime.js`: lightweight adapter for previewing the supplied
  template format outside Design Canvas; no installer dependencies.
- `build_preview.py`: regenerate `index.html` after editing the component.
- `verify_preview.py`: browser interaction checks (Python Playwright + Chromium;
  run while the local server is listening on port 8765).
- `previews/`: screenshots of the empty, chosen, and customized states.

The installer implementation remains Rust. These HTML/JavaScript files are the
supplied design format and a browser demonstration. Port the explicit selection
model and derived counts/sources into the Rust UI before connecting installation.

## Verified

Chromium interaction checks passed for the unselected entry, zero-app starter,
Everyday starter, combining/removing packs, per-app removal, search and empty
results, restoring suggestions, Escape dismissal, navigation persistence, matching
review totals, source requirements after changing distribution, and page widths.
Screenshots were visually inspected at the supplied 1280 × 800 canvas size.
The managed browser blocked file:// navigation, so execution was verified through
the included local HTTP preview. The standalone page embeds its template and
runtime and uses relative paths for bundled logos; Google fonts are optional.
