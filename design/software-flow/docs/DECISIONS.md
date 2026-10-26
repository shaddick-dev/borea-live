# Decisions and status

2026-10-26: Import this supplied backend as a separate Cargo workspace. The GitHub
repository already has crates with the same names and different APIs; replacing
those would discard existing functionality. Integration requires an explicit API
migration after the new backend is verified.

The backend plan is supplied project documentation, not proof of implementation
or package correctness. Its original 31 tests pass; package verification and VM
installation/boot checks are separate acceptance requirements.

The uploaded plan changes target channels, bootloaders, automatic region defaults
and encryption relative to earlier requirements. Those remain open integration
decisions. No disk-writing service is enabled by this import.
