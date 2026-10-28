#!/usr/bin/env bash
# Borea setup: from a fresh Linux install to borea-rataTUI, in one go.
#
#   curl -fsSL https://shaddick-dev.github.io/borea-live/install | sh
#
# That fetches scripts/install.sh, which fetches this script and runs it with
# the terminal attached (both are served by .github/workflows/pages.yml).
# From a checkout: bash scripts/borea-setup.sh
#
# Downloads the ready-made borea-rataTUI from the latest release (a static
# program that runs on any x86-64 distribution), checks its SHA-256, installs it
# into ~/.local/bin with an app launcher, and starts it. No packages, no Rust, no
# sudo. borea-rataTUI then downloads Borea Live itself and writes your USB stick.
#
# Only with --from-source it builds
# instead: installs your distribution's packages (asks once, uses sudo), Rust
# with rustup, clones Borea Live to ~/borea-live, and builds borea-rataTUI.
#
# Options:
#   --from-source  build borea-rataTUI from source instead of downloading it
#   --flash-only   with --from-source: skip the packages for building the ISO
#   --no-run       set everything up, but don't start borea-rataTUI
#   --dir PATH     with --from-source: where to keep the checkout (~/borea-live)
#   --yes          don't ask before installing packages
#   --dry-run      print what would run, change nothing
#
# Run it as your normal user. It never writes to a disk: choosing and erasing a
# USB stick happens in the helper, which asks you to type the stick's name.

set -euo pipefail

REPO_URL="https://github.com/shaddick-dev/borea-live"
DIR="${HOME}/borea-live"
FLASH_ONLY=0
FROM_SOURCE=0
RUN=1
YES=0
DRY=0
# Tests point this at a sample file.
OS_RELEASE="${BOREA_OS_RELEASE:-/etc/os-release}"

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die() {
    printf '\033[1;31merror:\033[0m %s\n' "$*" >&2
    exit 1
}
run() {
    printf '    $ %s\n' "$*"
    if [ "$DRY" -eq 0 ]; then "$@"; fi
}

while [ $# -gt 0 ]; do
    case "$1" in
    --flash-only) FLASH_ONLY=1 ;;
    --from-source) FROM_SOURCE=1 ;;
    --no-run) RUN=0 ;;
    --yes | -y) YES=1 ;;
    --dry-run) DRY=1 ;;
    --dir)
        [ $# -ge 2 ] || die "--dir needs a path"
        DIR="$2"
        shift
        ;;
    -h | --help)
        cat <<'USAGE'
Borea setup: install borea-rataTUI, then start it.

By default it downloads the ready-made borea-rataTUI from the latest release:
no packages, no Rust, no sudo.

  --from-source  build borea-rataTUI from source instead of downloading it
  --flash-only   with --from-source: skip the packages for building the ISO
  --no-run       set everything up, but don't start borea-rataTUI
  --dir PATH     with --from-source: where to keep the checkout (~/borea-live)
  --yes          don't ask before installing packages
  --dry-run      print what would run, change nothing
USAGE
        exit 0
        ;;
    *) die "unknown option: $1 (try --help)" ;;
    esac
    shift
done

# Questions go to the terminal even when the script itself arrives on stdin.
ask() {
    [ "$YES" -eq 1 ] && return 0
    local reply
    if [ -r /dev/tty ]; then
        read -r -p "$1 [Y/n] " reply </dev/tty
    else
        die "no terminal to ask on; rerun with --yes"
    fi
    case "$reply" in "" | [Yy]*) return 0 ;; *) return 1 ;; esac
}

[ "$(id -u)" -ne 0 ] || die "run this as your normal user, not root: it uses sudo only for packages, and the ISO build refuses root"

RELEASES="${BOREA_RELEASES_URL:-https://github.com/shaddick-dev/borea-live/releases/latest/download}"
helper="${XDG_BIN_HOME:-$HOME/.local/bin}/borea-rataTUI"

# borea-rataTUI --install already says if ~/.local/bin isn't on your PATH.
finish() {
    say "Done. Open \"borea-rataTUI\" from your app launcher, or run: $helper"
    if [ "$RUN" -eq 1 ]; then
        say "Starting borea-rataTUI: it downloads the latest Borea Live, then writes your USB stick"
        [ "$DRY" -eq 1 ] || exec "$helper"
    fi
    exit 0
}

# ── The quick way: the ready-made program ─────────────────────────────────────
# Returns non-zero when the release is unavailable. Never falls back to a build.
install_ready_made() {
    local name=borea-rataTUI-x86_64-linux tmp
    [ "$(uname -m)" = x86_64 ] || { warn "no ready-made borea-rataTUI for $(uname -m)"; return 1; }
    command -v curl >/dev/null || { warn "curl is needed to download borea-rataTUI"; return 1; }
    say "Downloading borea-rataTUI from the latest release"
    if [ "$DRY" -eq 1 ]; then
        printf '    $ curl %s/%s (+ .sha256), check it, then: %s --install\n' "$RELEASES" "$name" "$name"
        return 0
    fi
    tmp="$(mktemp -d)"
    if ! curl -fsL "$RELEASES/$name.sha256" -o "$tmp/$name.sha256"; then
        rm -rf "$tmp"
        warn "no ready-made borea-rataTUI has been published yet"
        return 1
    fi
    curl -fL --progress-bar "$RELEASES/$name" -o "$tmp/$name" || { rm -rf "$tmp"; die "the download failed; run this again to retry"; }
    (cd "$tmp" && sha256sum --quiet -c "$name.sha256") || { rm -rf "$tmp"; die "the download doesn't match its published SHA-256"; }
    chmod 755 "$tmp/$name"
    # It installs itself (borea-rataTUI and its older names) and its app launcher.
    "$tmp/$name" --install
    rm -rf "$tmp"
}

if [ "$FROM_SOURCE" -eq 0 ]; then
    install_ready_made && finish
    die "the ready-made download is unavailable. Check your connection and the releases at $REPO_URL/releases, then retry.
No compiler or build packages were installed. Developers can explicitly choose --from-source."
fi

[ -r "$OS_RELEASE" ] || die "can't read $OS_RELEASE to tell which distribution this is"

# ── 1. Which distribution ─────────────────────────────────────────────────────
ID="" ID_LIKE="" PRETTY_NAME=""
# shellcheck disable=SC1090
. "$OS_RELEASE"
family=""
for id in $ID $ID_LIKE; do
    case "$id" in
    arch) family=arch ;;
    fedora | rhel) family=fedora ;;
    debian | ubuntu) family=debian ;;
    suse | opensuse | opensuse-tumbleweed | opensuse-leap) family=suse ;;
    esac
    [ -n "$family" ] && break
done
say "This is ${PRETTY_NAME:-$ID}"

if [ "$DRY" -eq 0 ] && { [ -e /run/ostree-booted ] || [ "$ID" = steamos ]; }; then
    die "this system is image-based (Silverblue, Bazzite, SteamOS…), so packages can't be added directly.
Run this script inside a toolbox or distrobox, which shares your home folder:
    toolbox create && toolbox enter
then run it again with --no-run, and start ~/.local/bin/borea-rataTUI on the host."
fi

packages=()
case "$family" in
arch)
    packages=(base-devel git curl)
    [ "$FLASH_ONLY" -eq 1 ] || packages+=(perl python rsync cpio unzip bc wget file tar xz bzip2 which bison flex)
    install=(sudo pacman -S --needed --noconfirm)
    ;;
fedora)
    packages=(gcc git curl)
    [ "$FLASH_ONLY" -eq 1 ] || packages+=(make gcc-c++ patch perl python3 rsync cpio unzip bc wget file tar xz bzip2 which diffutils findutils bison flex)
    install=(sudo dnf install -y)
    ;;
debian)
    packages=(build-essential git curl ca-certificates)
    [ "$FLASH_ONLY" -eq 1 ] || packages+=(patch perl python3 rsync cpio unzip bc wget file tar xz-utils bzip2 bison flex)
    install=(sudo apt-get install -y)
    ;;
suse)
    packages=(gcc git curl)
    [ "$FLASH_ONLY" -eq 1 ] || packages+=(make gcc-c++ patch perl python3 rsync cpio unzip bc wget file tar xz bzip2 which diffutils bison flex)
    install=(sudo zypper --non-interactive install)
    ;;
*)
    warn "no package list for '$ID'. Install a C compiler, git and curl yourself"
    [ "$FLASH_ONLY" -eq 1 ] || warn "for the ISO build also: make g++ patch perl python3 rsync cpio unzip bc wget file tar xz bzip2 bison flex (or install Nix, which the build uses for anything missing)"
    ;;
esac

# ── 2. System packages ────────────────────────────────────────────────────────
if [ ${#packages[@]} -gt 0 ]; then
    say "Packages for $family: ${packages[*]}"
    if ask "Install them with sudo?"; then
        [ "$family" = debian ] && run sudo apt-get update
        run "${install[@]}" "${packages[@]}"
    else
        warn "skipped; the build fails later if anything is missing"
    fi
fi

# ── 3. Rust ───────────────────────────────────────────────────────────────────
# Borea's GUI crates need Rust 1.95 or newer; most distributions ship older, so
# rustup it is. It lives in ~/.cargo and ~/.rustup and needs no root.
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
if command -v rustup >/dev/null 2>&1; then
    say "Updating Rust"
    run rustup update stable
    # rustup can be present with no default toolchain; then rustc and cargo fail.
    rustup default >/dev/null 2>&1 || run rustup default stable
else
    say "Installing Rust with rustup"
    if [ "$DRY" -eq 0 ]; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
        . "$HOME/.cargo/env"
    else
        printf '    $ curl https://sh.rustup.rs | sh -s -- -y --profile minimal\n'
    fi
fi
if [ "$DRY" -eq 0 ]; then
    version="$(rustc --version | awk '{print $2}')"
    minor="${version#1.}"
    minor="${minor%%.*}"
    [ "$minor" -ge 95 ] 2>/dev/null || die "rustc $version is too old (Borea needs 1.95+); run: rustup default stable"
    say "Rust $version"
fi

# ── 4. The code ───────────────────────────────────────────────────────────────
if [ -d "$DIR/.git" ]; then
    say "Updating $DIR"
    run git -C "$DIR" pull --ff-only
elif [ -e "$DIR" ]; then
    die "$DIR exists but isn't a git checkout; move it aside or pass --dir"
else
    say "Cloning Borea Live into $DIR"
    run git clone "$REPO_URL" "$DIR"
fi

# ── 5. borea-rataTUI ───────────────────────────────────────────────────────
say "Building and installing borea-rataTUI (under a minute on most machines the first time, seconds after)"
run cargo run --profile app --locked --manifest-path "$DIR/Cargo.toml" -p borea-flash -- --install

finish
