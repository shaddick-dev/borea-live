# borea-rataTUI

Prepare the Borea Live ISO and write it as a bootable USB drive from one terminal
interface. The commands share the same writer:

- `borea-rataTUI`: build if the ISO is missing or the checkout changed, then flash.
- `borea-rataTUI IMAGE.iso`: flash an existing image directly.
- `borea-rataTUI --build`: force an ISO rebuild, then flash.
- `borea-rataTUI --list [IMAGE.iso]`: list USB candidates without building or writing.
- `borea-flash [IMAGE.iso]`: compatibility command; offers a rebuild with **b**.

## Install and launch

From this checkout, as your ordinary user:

```sh
cargo run --release -p borea-flash -- --install
borea-rataTUI
```

Installation adds `borea-rataTUI` to `~/.local/bin` (or the absolute
`XDG_BIN_HOME`), plus its earlier names `borea-helper` and `borea-flash`, and
installs the **borea-rataTUI** app launcher in place of the old one.
The launcher opens the installed binary directly in a terminal. Reinstall after
updating the code. No Rust compiler runs just to open the installed interface.
The ISO build still needs Cargo and the host tools described in `live/README.md`.

The helper remembers the checkout it was compiled from and works from other
directories while that checkout exists. To run without installation:

```sh
cargo run -p borea-flash --bin borea-rataTUI
```

## Workflow

1. Prepare the ISO. Missing or changed sources start the build with progress and
   a log. The first build can take up to a few hours, depending on your machine; later builds are incremental.
2. Check the ISO and its adjacent SHA-256 when present.
3. Select a USB drive. **r** rescans; **u** unmounts a mounted candidate.
4. Type the selected device name to confirm erasing it.
5. Write the image raw to the whole drive, flush it, then read every byte back.
6. **e** ejects the verified drive when supported.

The raw copy is equivalent to `dd`; it does not copy the ISO as a file into a
filesystem. The Rust writer adds progress, cancellation and verification.
Only USB/removable candidates are offered; the running system's disk and the
disk holding the source ISO are excluded.

On a desktop the helper requests device access through UDisks2/polkit. With no
desktop authorization agent, an existing ISO can be written from a root terminal
using `sudo /absolute/path/to/borea-rataTUI IMAGE.iso`. Build the ISO as an ordinary
user first; the ISO builder refuses root.
