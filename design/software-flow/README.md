# Borea · system profiles and apps

This finishes the app/profile flow from the supplied `borea-all.zip`.

## Try the flow

Open **index.html**. If local files are restricted, run:

```sh
python3 -m http.server 8766
```

Then open http://localhost:8766. The preview starts at page 05.
Use `?start=network` to explore the surrounding ten-screen prototype.
This is a design preview; Install simulates progress and does not alter disks.

## The finished flow

1. **System profile:** explicitly choose Minimal install, Server, Server with GUI,
   or Workstation. Nothing starts selected. Desktop profiles use the desktop
   chosen on step 04; headless profiles skip the app screen.
2. **Apps:** explicitly choose No extra apps or Everyday. Everyday reads the
   seven-app Essentials set from the Rust catalog.
3. Optionally combine Gaming, Creator and Developer. Each uses the catalog's
   actual apps and associated tool add-ons, including Gaming Tweaks and the
   Developer compiler/container tools. No separate hardcoded app lists.
4. **Customize apps** opens category browsing, search and individual controls.
   **Review selected apps** shows the complete selected list, including role apps.
5. **Roles & tools** live behind a button on the system profile screen. The
   package tree is available under **Customize packages**, rather than dominating
   page 05. Boot essentials and automatic drivers stay protected.
6. **Sources & install preference** is a separate dialog. It shows only the chosen
   distro's sources, AUR only for Arch, Flathub and upstream AppImage fallback.
   Driver-required sources stay enabled. Unresolved app choices are visible and
   block Continue/Install until corrected.
7. Review uses the same app, package and driver selection. Duplicate package
   names within a source count once. App sizes are rough installed-size estimates.

## Selection behavior

Starter apps and enabled app profiles form one union; explicit per-app overrides
are applied afterwards. Turning off a profile removes its suggested apps without
freezing other profiles' selections. Intentional overrides survive profile changes.
Restore profile apps clears app overrides. Roles use the same pattern: profile
suggestions followed by explicit role overrides. Role apps are shown as included
and are edited through their role/package controls. Headless profiles keep app
choices dormant, so switching back to a desktop restores them.

Back from the app screen returns to the system profile screen. Back from Disk
returns to the last software screen. Returning from Review preserves all choices.
Dialogs support Escape and keyboard focus stays within the open dialog.

## Files and editing

- `design/Prototype.dc.html`: updated, editable Design Canvas component.
- `design/*.dc.html`, `design/canvas.json`: supplied boards, with the renamed
  Software board still importing the same component.
- `crates/borea-catalog/`: supplied Rust catalog, groups, hardware and driver rules.
  The preparatory Rust 2024 workspace setup is retained; its 31 tests pass.
- `design/preview-runtime.js`: browser adapter for the canvas template format.
- `build_preview.py`: rebuild the standalone page from the editable component.
- `verify_flow.py`: Chromium interaction checks; needs Playwright, Chromium and
  the local server on port 8766.
- `previews/`: screenshots of profiles, apps, customization, packages and Review.
- `docs/BACKEND_PLAN.md`: supplied future backend plan, outside this UI task.

The component's catalog JSON is identical to `cargo run --locked -p borea-catalog
--example export_json`. If Rust data changes, regenerate the inline `CATALOG`
value from that export, then run `python3 build_preview.py`. The earlier backend
setup notes are in `docs/IMPORT-NOTES.md`; the full backend is not implemented.

## Verification

Browser checks cover explicit profile/app choices, all app profiles and their
associated tools, per-app override/removal/reset, search, role-supplied apps,
package edits, headless skipping, navigation persistence, source locks, matching
Review totals, and blocked continuation for unresolved sources. An 80-case matrix
covers four distros × four environments × five supplied hardware fixtures.
The Rust JSON export matches the embedded design catalog exactly. Screenshots
were inspected at 1280 × 800. The Rust catalog's 31 tests, Clippy and formatting
checks passed during workspace preparation. These checks do not certify package
availability or an installed operating system.
