# Borea — distro-agnostic live installer

- `design/` — the design canvas files: `Prototype.dc.html` (all 10 screens, clickable), one board per screen,
  `Main.dc.html` (visual language), `Flow.dc.html` (flow), `Logic.dc.html` (Rust architecture), `canvas.json`.
  `logos/` holds the official distro logos used unaltered. (In the canvas they're served as uploaded assets.)
- `crates/borea-catalog/` — the Rust crate: sources, apps, groups, hardware probe, automatic drivers, `borea-probe`
  binary and its systemd unit. `cargo test` runs 31 tests. `AGENTS.md` points coding agents to the plan.
- `docs/BACKEND_PLAN.md` — the full backend plan for Codex.
- `docs/drivers-policy.md` — how drivers are chosen per GPU and distro.


## Workspace setup

The uploaded catalog is now a Rust 2024 workspace (declared MSRV 1.85).
Run `cargo fmt --all --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`,
and `cargo test --locked --workspace` from this folder. CI runs those same checks.

The original 31 catalog tests still pass. The runtime `expect` in app source
selection was replaced with an error path; failed source activation cannot add
an app. Rust 2024's reserved `gen` identifier was migrated to `generation`.

This completes the workspace setup milestone (M0), not the installation backend.
The existing upstream/package claims remain unverified; no installation or VM
boot certification is asserted. `docs/DECISIONS.md` records integration differences
from the earlier GitHub project. Backend implementation starts at M1 in the plan.
