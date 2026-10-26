#!/bin/bash
# CI-only metadata checks inside an official disposable distribution container.
set -euo pipefail
case "$BOREA_AUDIT_DISTRO" in
 arch)
  pacman-key --init
  pacman-key --populate archlinux
  printf '\n[multilib]\nInclude = /etc/pacman.d/mirrorlist\n' >> /etc/pacman.conf
  pacman -Sy --noconfirm
  ;;
 debian)
  apt-get update
  apt-get install -y --no-install-recommends ca-certificates
  sed -i -e 's|http:|https:|g' -e 's/Components: main$/Components: main contrib non-free non-free-firmware/' /etc/apt/sources.list.d/debian.sources
  apt-get update
  ;;
 fedora)
  for kind in free nonfree; do
   rpm --import "https://download1.rpmfusion.org/$kind/fedora/RPM-GPG-KEY-rpmfusion-$kind-fedora-44"
   dnf --setopt=localpkg_gpgcheck=1 install -y "https://download1.rpmfusion.org/$kind/fedora/rpmfusion-$kind-release-44.noarch.rpm"
  done
  dnf makecache --refresh
  ;;
 opensuse)
  zypper --non-interactive refresh
  zypper --non-interactive install openSUSE-repos-NVIDIA
  rpm --import https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Tumbleweed/repodata/repomd.xml.key
  zypper --non-interactive addrepo --refresh --priority 90 https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Tumbleweed/ packman
  zypper --non-interactive refresh
  ;;
 *) exit 2 ;;
esac
exec /verify/borea-verify "$BOREA_AUDIT_DISTRO" --check-native
