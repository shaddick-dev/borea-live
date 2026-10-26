FROM debian:trixie
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    qemu-system-x86 qemu-utils ovmf linux-image-amd64 busybox-static cpio \
    debootstrap debian-archive-keyring cryptsetup btrfs-progs dosfstools e2fsprogs \
    ntfs-3g gparted parted gdisk util-linux udev iproute2 efibootmgr skopeo umoci dbus \
    curl ca-certificates gnupg tar xz-utils zstd gzip perl gcc libc6-dev musl-tools mtools && rm -rf /var/lib/apt/lists/*
