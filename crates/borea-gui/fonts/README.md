# Fonts

Windpad and Borea Live draw every word and icon with JetBrains Mono Nerd Font,
built into the binaries so they look the same on any live session.

| File | From |
| --- | --- |
| `JetBrainsMonoNerdFont-Regular.ttf` | Nerd Fonts v3.5.1, `JetBrainsMono.tar.xz` |
| `JetBrainsMonoNerdFont-Bold.ttf` | the same archive |
| `OFL.txt` | the same archive: the SIL Open Font License 1.1 both fonts are under |

The archive's SHA-256 is
`04d5e8f903693f9dd13e16f867e994834e681eb3c72c0d337a770dcda09010cf`, as listed in
the release's `SHA-256.txt`
(<https://github.com/ryanoasis/nerd-fonts/releases/tag/v3.5.1>).

To update: download a newer `JetBrainsMono.tar.xz`, check it against that
release's `SHA-256.txt`, and replace the two fonts and `OFL.txt`. The icons the
interface uses are listed in `src/icon.rs`, and its test fails if a font no
longer has one of them.
