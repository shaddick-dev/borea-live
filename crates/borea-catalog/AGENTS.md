# Instructions for coding agents (Codex)

Read `docs/BACKEND_PLAN.md` first and follow it: it is the contract for the Borea backend.

- Work milestone by milestone (§12). Keep CI green: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- Never break the hard constraints in §0 (no NixOS, online only, only the chosen distro's repos + AUR on Arch + Flathub + AppImage last resort, nothing written before Install, drivers automatic, secrets never logged).
- `borea-catalog` is the single source of truth for apps, groups, sources, hardware and drivers. Extend it in place; don't copy its data.
- No `unwrap`/`expect`/`panic!` outside tests. Commands are argv vectors (`CommandSpec`), never shell strings. Secrets reach commands only through stdin.
- Anything marked *verify* must be checked against the distro's live package index and recorded in `docs/VERIFIED.md`.
- Record any decision the plan leaves open in `docs/DECISIONS.md` before relying on it.
