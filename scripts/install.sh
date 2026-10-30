#!/bin/sh
# Borea, in one line, on any Linux distribution:
#
#   curl -fsSL https://shaddick-dev.github.io/borea-live/install | sh
#
# Options go after `sh -s --`, e.g. `... | sh -s -- --from-source`.
#
# This is plain POSIX sh, so it runs under dash, busybox or bash alike. Piped
# into sh, its standard input is this script, so it fetches the full setup
# (scripts/borea-setup.sh) and runs that with the terminal attached: the setup
# and the borea-rataTUI app it starts both need your keyboard.
set -eu

url="${BOREA_SETUP_URL:-https://shaddick-dev.github.io/borea-live/setup}"

command -v bash >/dev/null 2>&1 || { echo "Borea needs bash; install it with your package manager, then run this again." >&2; exit 1; }
command -v curl >/dev/null 2>&1 || { echo "Borea needs curl." >&2; exit 1; }

tmp="$(mktemp "${TMPDIR:-/tmp}/borea-setup.XXXXXX")"
trap 'rm -f "$tmp"' EXIT
curl -fsSL "$url" -o "$tmp" || { echo "Could not download the Borea setup from $url" >&2; exit 1; }

if [ -r /dev/tty ] && (: </dev/tty) 2>/dev/null; then
    bash "$tmp" "$@" </dev/tty
else
    bash "$tmp" "$@"
fi
