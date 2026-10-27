#!/usr/bin/env bash
# Publish a Borea Live release from an ISO you built, so users download it in
# minutes instead of building for hours.
#
#   scripts/publish-release.sh                 # newest ISO in target/live/out
#   scripts/publish-release.sh path/to.iso
#
# Creates the GitHub release (tag v<workspace version>-<date>) with the ISO as
# borea-live-x86_64.iso and its .sha256. Publishing triggers
# .github/workflows/release.yml, which attaches the static borea-rataTUI binary
# and, because the ISO is already there, doesn't build another.
# Needs the GitHub CLI, signed in with write access: gh auth login

set -euo pipefail
cd "$(dirname "$0")/.."

iso="${1:-$(ls -t target/live/out/borea-live-*.iso 2>/dev/null | head -n1 || true)}"
[ -n "$iso" ] && [ -f "$iso" ] || { echo "No ISO found; build one with: cargo run --release -p borea-build -- iso" >&2; exit 1; }
# GitHub's limit for one release file.
[ "$(stat -c %s "$iso")" -lt 2147483648 ] || { echo "$iso is over GitHub's 2 GiB limit for a release file" >&2; exit 1; }
command -v gh >/dev/null || { echo "Install the GitHub CLI (gh) and run: gh auth login" >&2; exit 1; }

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
tag="v${version}-$(date +%Y%m%d%H%M)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
ln -s "$(realpath "$iso")" "$stage/borea-live-x86_64.iso"
(cd "$stage" && sha256sum borea-live-x86_64.iso >borea-live-x86_64.iso.sha256)

echo "Publishing $iso as $tag ($(du -h "$iso" | cut -f1))"
gh release create "$tag" "$stage/borea-live-x86_64.iso" "$stage/borea-live-x86_64.iso.sha256" \
    --title "Borea Live $tag" \
    --notes "Borea Live, built from $(git rev-parse --short HEAD). Write it to a USB stick with borea-rataTUI, which downloads it for you."
echo "Done. The release workflow now attaches borea-rataTUI; users get both from releases/latest."
