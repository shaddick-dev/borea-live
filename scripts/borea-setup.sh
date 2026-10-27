#!/usr/bin/env bash
# Borea setup: from a fresh Linux install to Borea TUI Helper, in one go.
#
#   bash <(curl -fsSL https://raw.githubusercontent.com/shaddick-dev/borea-live/main/scripts/borea-setup.sh)
#
# 1. Installs the system packages for your distribution (asks once, then uses sudo).
# 2. Installs Rust with rustup, in your home folder, or updates it.
# 3. Clones Borea Live to ~/borea-live, or fast-forwards an existing clone.
# 4. Installs Borea TUI Helper (borea-helper) and its app launcher.
# 5. Starts the helper: it builds the ISO when needed, then writes your USB stick.
#
# Options:
#   --flash-only   skip the ISO build packages (you already have an ISO)
#   --no-run       set everything up, but don't start the helper
#   --dir PATH     where to keep the checkout (default: ~/borea-live)
#   --yes          don't ask before installing packages
#   --dry-run      print what would run, change nothing
#
# Run it as your normal user. It never writes to a disk: choosing and erasing a
# USB stick happens in the helper, which asks you to type the stick's name.

set -euo pipefail

REPO_URL="https://github.com/shaddick-dev/borea-live"
DIR="${HOME}/borea-live"
FLASH_ONLY=0
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
Borea setup: install everything Borea TUI Helper needs, then start it.

  --flash-only   skip the ISO build packages (you already have an ISO)
  --no-run       set everything up, but don't start the helper
  --dir PATH     where to keep the checkout (default: ~/borea-live)
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
then run it again with --no-run, and start ~/.local/bin/borea-helper on the host."
fi

packages=()
case "$family" in
arch)
    packages=(base-devel git curl)
    [ "$FLASH_ONLY" -eq 1 ] || packages+=(perl python rsync cpio unzip bc wget file tar xz bzip2 which)
    install=(sudo pacman -S --needed --noconfirm)
    ;;
fedora)
    packages=(gcc git curl)
    [ "$FLASH_ONLY" -eq 1 ] || packages+=(make gcc-c++ patch perl python3 rsync cpio unzip bc wget file tar xz bzip2 which diffutils findutils)
    install=(sudo dnf install -y)
    ;;
debian)
    packages=(build-essential git curl ca-certificates)
    [ "$FLASH_ONLY" -eq 1 ] || packages+=(patch perl python3 rsync cpio unzip bc wget file tar xz-utils bzip2)
    install=(sudo apt-get install -y)
    ;;
suse)
    packages=(gcc git curl)
    [ "$FLASH_ONLY" -eq 1 ] || packages+=(make gcc-c++ patch perl python3 rsync cpio unzip bc wget file tar xz bzip2 which diffutils)
    install=(sudo zypper --non-interactive install)
    ;;
*)
    warn "no package list for '$ID'. Install a C compiler, git and curl yourself"
    [ "$FLASH_ONLY" -eq 1 ] || warn "for the ISO build also: make g++ patch perl python3 rsync cpio unzip bc wget file tar xz bzip2 (or install Nix, which the build uses for anything missing)"
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

# ── 5. Borea TUI Helper ───────────────────────────────────────────────────────
say "Building and installing Borea TUI Helper (a few minutes the first time)"
run cargo run --release --locked --manifest-path "$DIR/Cargo.toml" -p borea-flash -- --install

helper="${XDG_BIN_HOME:-$HOME/.local/bin}/borea-helper"
case ":$PATH:" in
*":$(dirname "$helper"):"*) ;;
*) warn "$(dirname "$helper") isn't on your PATH; add this to ~/.profile: export PATH=\"$(dirname "$helper"):\$PATH\"" ;;
esac

say "Done. Open \"Borea TUI Helper\" from your app launcher, or run: $helper"
if [ "$RUN" -eq 1 ]; then
    if [ "$FLASH_ONLY" -eq 1 ]; then
        # borea-flash writes the newest borea-live*.iso it finds, never building one.
        say "Starting the writer: it uses the newest ISO in $DIR/target/live/out, this folder, or ~/Downloads"
        [ "$DRY" -eq 1 ] || exec "$(dirname "$helper")/borea-flash"
    else
        say "Starting the helper: it builds Borea Live if needed (hours the first time), then writes your USB stick"
        [ "$DRY" -eq 1 ] || exec "$helper"
    fi
fi
