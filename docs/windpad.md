# Windpad

Windpad and Borea Live share the [Iced interface](iced-ui.md).

| Page | Choices |
| --- | --- |
| Connect | Wi-Fi, hidden networks, connection status |
| Machine | Detected hardware and installation findings |
| Family | Arch, Fedora, Debian, openSUSE |
| Distribution | Upstream target, edition and image variant |
| Desktop | Compositor, shell and optional extras |
| Review | Supported installation path, storage, account, region and approval |

- Nothing selects a distribution or later choice for the user.
- Returning to an earlier choice invalidates later approval.
- Native Arch/CachyOS installation and upstream Omarchy handoff keep their existing Rust services.
- Unsupported combinations stay previews. A catalog entry alone never enables disk writes.
- Whole-drive and Windows-alongside layouts are explicit choices.
- Extra applications remain optional and finish quietly after graphical login.
- Firmware setup is available from the footer with a restart confirmation.

[Design rules](design.md) · [Native Arch/CachyOS path](arch-noctalia.md) · [Architecture](architecture.md)
