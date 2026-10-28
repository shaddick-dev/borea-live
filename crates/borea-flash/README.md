# borea-rataTUI

Get the Borea Live ISO and write it as a bootable USB drive from one terminal
interface. The commands share the same writer:

- `borea-rataTUI`: download the latest release's ISO (or reuse the copy already
  downloaded when it's still the latest), then flash. No local compilation.
- `borea-rataTUI IMAGE.iso`: flash an existing image directly.
- `borea-rataTUI --cached`: use an existing local ISO without downloading or compiling.
- `borea-rataTUI --compose`: create the ISO from prebuilt Linux components and
  the current Borea Rust apps; Docker, Rust and a checkout required. No release needed.
- `borea-rataTUI --update`: replace the app with the latest release's ready-made
  build, in seconds.
- `borea-rataTUI --make`: developers, in a checkout: build the ISO from source if
  the sources changed, then flash. `--build` always rebuilds.
- `borea-rataTUI --list [IMAGE.iso]`: list USB candidates without building or writing.
- `borea-flash [IMAGE.iso]`: compatibility command; writes an existing ISO or downloads one.

An unavailable release offers local composition with **b** when a checkout is
available. It downloads prebuilt Linux components rather than compiling Linux.
Source builds of the whole runtime still require explicit `--make` or `--build`.

## Install and launch

Install the published app as your ordinary user, without Rust or build packages:

```sh
curl -fsSL https://shaddick-dev.github.io/borea-live/install | sh
```

Setup stops if the release is unavailable. `--from-source` explicitly enables
developer setup; it is never an automatic fallback. Developers can also build
from a checkout with the lighter `app` compilation profile:

```sh
cargo run --profile app -p borea-flash -- --install
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

1. Get the ISO. By default it downloads `borea-live-x86_64.iso` from the latest
   GitHub release into `~/.cache/borea`, resuming an interrupted download and
   checking it against the release's SHA-256; the next run only fetches the
   checksum. With `--make`/`--build` it builds from source instead (the first
   build can take up to a few hours; later builds are incremental).
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

## Releases

Users download, so someone has to publish. `.github/workflows/release.yml` runs
when a GitHub release is published: it attaches a static
`borea-rataTUI-x86_64-linux` (what `scripts/borea-setup.sh` installs) and builds
`borea-live-x86_64.iso` unless the release already has one. To publish an ISO
you built locally, which is quicker than waiting for CI:

```sh
scripts/publish-release.sh            # newest ISO in target/live/out
```
