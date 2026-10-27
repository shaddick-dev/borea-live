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
  if [ -n "${SSL_CERT_FILE:-}" ]; then printf 'Acquire::https::CaInfo "%s";\n' "$SSL_CERT_FILE" > /etc/apt/apt.conf.d/90-borea-session-ca; fi
  apt-get update
  apt-get install -y --no-install-recommends ca-certificates
  sed -i -e 's|http:|https:|g' -e 's/Components: main$/Components: main contrib non-free non-free-firmware/' /etc/apt/sources.list.d/debian.sources
  dpkg --add-architecture i386
  apt-get update
  ;;
 fedora)
  if [ -n "${SSL_CERT_FILE:-}" ]; then printf "\nsslcacert=%s\nproxy_sslcacert=%s\n" "$SSL_CERT_FILE" "$SSL_CERT_FILE" >> /etc/dnf/dnf.conf; fi
  for kind in free nonfree; do
   rpm --import "https://download1.rpmfusion.org/$kind/fedora/RPM-GPG-KEY-rpmfusion-$kind-fedora-2020"
   dnf --setopt=localpkg_gpgcheck=1 install -y "https://download1.rpmfusion.org/$kind/fedora/rpmfusion-$kind-release-44.noarch.rpm"
  done
  dnf makecache --refresh
  ;;
 opensuse)
  sed -i 's|http://|https://|g' /etc/zypp/repos.d/*.repo
  zypper --non-interactive refresh
  zypper --non-interactive install openSUSE-repos-NVIDIA
  rpm --import https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Tumbleweed/repodata/repomd.xml.key
  zypper --non-interactive addrepo --refresh --priority 90 https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Tumbleweed/ packman
  zypper --non-interactive refresh
  ;;
 *) exit 2 ;;
esac
exec /verify/borea-verify "$BOREA_AUDIT_DISTRO" --check-native
