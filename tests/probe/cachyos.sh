#!/bin/bash
# What CachyOS's own repositories give a Noctalia install on an x86-64-v3 CPU,
# configured the way CachyOS configures itself.
set -u
keys=/usr/share/pacman/keyrings
cp backend/../crates/borea-backend/data/cachyos/cachyos.gpg "$keys/"
cp crates/borea-backend/data/cachyos/cachyos-trusted crates/borea-backend/data/cachyos/cachyos-revoked "$keys/"
pacman-key --init >/dev/null; pacman-key --populate archlinux cachyos >/dev/null
printf 'Server = https://cdn77.cachyos.org/repo/$arch/$repo\n' > /etc/pacman.d/cachyos-mirrorlist
printf 'Server = https://cdn77.cachyos.org/repo/$arch_v3/$repo\n' > /etc/pacman.d/cachyos-v3-mirrorlist
cat > /etc/pacman.conf <<'CONF'
[options]
Architecture = x86_64 x86_64_v3
SigLevel = Required DatabaseOptional
ParallelDownloads = 8
[cachyos-v3]
Include = /etc/pacman.d/cachyos-v3-mirrorlist
[cachyos-core-v3]
Include = /etc/pacman.d/cachyos-v3-mirrorlist
[cachyos-extra-v3]
Include = /etc/pacman.d/cachyos-v3-mirrorlist
[cachyos]
Include = /etc/pacman.d/cachyos-mirrorlist
[core]
Include = /etc/pacman.d/mirrorlist
[extra]
Include = /etc/pacman.d/mirrorlist
[multilib]
Include = /etc/pacman.d/mirrorlist
CONF
pacman -Sy >/dev/null || { echo "SYNC FAILED"; pacman -Sy; }
echo "== Where each package comes from"
for p in base zlib zlib-ng-compat systemd glibc mangowm scenefx scenefx0.5 hyprland niri noctalia noctalia-greeter greetd linux-cachyos cachyos-settings cachyos-hooks cachyos-keyring cachyos-mirrorlist cachyos-v3-mirrorlist xdg-desktop-portal-wlr foot plymouth; do
  printf '%s: ' "$p"; pacman -Si "$p" 2>/dev/null | awk -F': ' '/^Repository/{r=$2}/^Version/{v=$2}/^Depends On/{d=$2}END{print (r?r" "v:"NOT FOUND")}'
done
pacman -Si mangowm | grep -E "^(Repository|Depends On)"
echo "== Install a Noctalia + MangoWC root, then -Syu as a user would"
root=/tmp/root; mkdir -p "$root/var/lib/pacman"
pacman -r "$root" -Sy --noconfirm --noscriptlet --hookdir /nonexistent \
  base linux-cachyos cachyos-settings cachyos-hooks cachyos-keyring cachyos-mirrorlist cachyos-v3-mirrorlist \
  mangowm xdg-desktop-portal-wlr noctalia noctalia-greeter greetd foot >/tmp/install.log 2>&1 \
  && echo "install OK" || { echo "install FAILED"; tail -20 /tmp/install.log; }
pacman -r "$root" -Q zlib zlib-ng-compat scenefx scenefx0.5 mangowm glibc systemd 2>&1
for p in glibc systemd mangowm; do printf '%s from: ' "$p"; grep -m1 -E "downloading $p-[0-9]" /tmp/install.log || echo "?"; done
pacman -r "$root" -Syu --noconfirm --noscriptlet --hookdir /nonexistent >/tmp/upgrade.log 2>&1 \
  && echo "-Syu OK: $(grep -cE '^ *\(' /tmp/upgrade.log) steps" || { echo "-Syu FAILED"; tail -15 /tmp/upgrade.log; }
