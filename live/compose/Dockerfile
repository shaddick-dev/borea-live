# syntax=docker/dockerfile:1
# Prebuilt runtime components, independent of the selected installation target.
# This is a package source for the Borea live host, not a Debian target installer.
FROM debian:13-slim
ENV DEBIAN_FRONTEND=noninteractive
RUN --mount=type=secret,id=proxy_ca \
    set -eu; \
    if [ -f /run/secrets/proxy_ca ]; then \
        printf 'Acquire::https::CaInfo "/run/secrets/proxy_ca";\n' > /etc/apt/apt.conf.d/99-borea-build-ca; \
    fi; \
    sed -i 's/Components: main$/Components: main contrib non-free non-free-firmware/' /etc/apt/sources.list.d/debian.sources; \
    printf '#!/bin/sh\nexit 101\n' > /usr/sbin/policy-rc.d; chmod 755 /usr/sbin/policy-rc.d; \
    # kexec-tools is only for handing off to other installers; it must never turn
    # an ordinary restart into a kexec of the live kernel.
    echo 'kexec-tools kexec-tools/load_kexec boolean false' | debconf-set-selections; \
    # The kernel and firmware come from trixie-backports: many USB Wi-Fi
    # adapters (Realtek RTL8812AU/8821AU since 6.14, RTL8851BU/8852BU since
    # 6.17, newer MediaTek) only have in-kernel drivers in newer kernels, and a
    # desktop with no internal Wi-Fi must still get online to install.
    printf 'Types: deb\nURIs: http://deb.debian.org/debian\nSuites: trixie-backports\nComponents: main contrib non-free non-free-firmware\nSigned-By: /usr/share/keyrings/debian-archive-keyring.gpg\n' \
        > /etc/apt/sources.list.d/backports.sources; \
    apt-get update; \
    apt-get install -y --no-install-recommends -t trixie-backports \
        linux-image-amd64 firmware-linux-free firmware-iwlwifi firmware-realtek \
        firmware-atheros firmware-brcm80211 firmware-mediatek firmware-misc-nonfree \
        firmware-ath9k-htc firmware-carl9170 firmware-libertas zd1211-firmware \
        firmware-amd-graphics firmware-intel-graphics intel-microcode amd64-microcode; \
    apt-get install -y --no-install-recommends \
        systemd systemd-sysv systemd-resolved libpam-systemd libnss-systemd dbus \
        network-manager wpasupplicant wireless-regdb iw ca-certificates \
        usb-modeswitch usb-modeswitch-data \
        cage libegl-mesa0 libgl1-mesa-dri libwayland-egl1 libxkbcommon0 xkb-data \
        fonts-dejavu-core polkitd libpam-modules udisks2 \
        cryptsetup btrfs-progs ntfs-3g gnupg gpgv efibootmgr parted fdisk \
        dosfstools e2fsprogs util-linux kmod busybox-static kexec-tools \
        debootstrap debian-archive-keyring skopeo umoci \
        tar xz-utils zstd openssl erofs-utils xorriso mtools \
        build-essential pkg-config; \
    rm -f /etc/apt/apt.conf.d/99-borea-build-ca; \
    rm -rf /var/lib/apt/lists/*; \
    printf 'Borea composition runtime\n' > /etc/borea-compose-runtime
