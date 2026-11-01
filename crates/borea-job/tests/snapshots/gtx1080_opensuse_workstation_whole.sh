#!/usr/bin/env bash
# REVIEW ONLY: secrets and runtime identifiers are placeholders.
# Execution is exclusively through the approved privileged service.
exit 64

set -euo pipefail

# Verifying the native installation tools.
# HTTPS GET https://example.org/opensuse.tar.xz -> /run/borea/tools/opensuse.tar; verify SHA256 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa before unpacking.
# HTTPS GET https://example.org/image.sig -> /run/borea/tools/opensuse.sig
'/usr/bin/gpgv' '--keyring' '/usr/share/keyrings/borea/opensuse.gpg' '/run/borea/tools/opensuse.sig' '/run/borea/tools/opensuse.tar'
'/usr/bin/tar' '-tf' '/run/borea/tools/opensuse.tar'
# Archive format is detected only after authentication; the matching branch runs:
'/usr/bin/tar' '--extract' '--file' '/run/borea/tools/opensuse.tar' '--directory' '/run/borea/tools/opensuse' '--strip-components=0'
'/usr/bin/skopeo' 'copy' '<docker-archive-or-oci-archive>:/run/borea/tools/opensuse.tar' 'oci:/run/borea/tools/opensuse.oci:borea'
'/usr/bin/umoci' 'unpack' '--image' '/run/borea/tools/opensuse.oci:borea' '/run/borea/tools/opensuse.bundle'
# For container archives move the verified bundle/rootfs into the tool root. Mount proc, sys and dev there; native tooling must exist. Temporary session CA trust is copied into /run.
# Check live utilities and the static x86-64 target helper before disk changes.
chroot '/run/borea/tools/opensuse' '/usr/bin/zypper' '--gpg-auto-import-keys' '--non-interactive' 'refresh'
chroot '/run/borea/tools/opensuse' '/usr/bin/zypper' '--non-interactive' 'install' 'openSUSE-repos-NVIDIA'
chroot '/run/borea/tools/opensuse' '/usr/bin/zypper' '--non-interactive' 'refresh'
chroot '/run/borea/tools/opensuse' '/usr/bin/zypper' '--non-interactive' 'install' '--dry-run' '--auto-agree-with-licenses' 'nvidia-compute-utils-G06' 'nvidia-driver-G06-kmp-meta' 'nvidia-gl-G06' 'nvidia-video-G06' 'NetworkManager' 'bluez' 'btrfsprogs' 'cryptsetup' 'cups' 'dracut' 'firewalld' 'glibc-locale' 'kbd' 'kernel-default' 'kernel-firmware-all' 'nano' 'patterns-kde-kde_plasma' 'pipewire' 'sdbootutil' 'sddm' 'shadow' 'shim' 'snapper' 'sudo' 'systemd' 'systemd-boot' 'ucode-amd' 'util-linux-systemd' 'zram-generator'

# Clearing the drive.
'/usr/sbin/sgdisk' '--zap-all' '/dev/vda'
'/usr/sbin/sgdisk' '--new=1:2048:2099199' '--typecode=1:ef00' '--change-name=1:EFI System' '/dev/vda'
'/usr/sbin/sgdisk' '--new=2:2099200:209715166' '--typecode=2:8309' '--change-name=2:borea_root' '/dev/vda'
'/usr/sbin/partprobe' '/dev/vda'
'/usr/bin/udevadm' 'settle'
'/usr/sbin/mkfs.fat' '-F' '32' '/dev/vda1'

# Locking your space.
# stdin: <redacted secret>
'/usr/sbin/cryptsetup' 'luksFormat' '--batch-mode' '--type' 'luks2' '--pbkdf' 'argon2id' '--cipher' 'aes-xts-plain64' '--key-size' '512' '--label' 'borea_root' '--key-file' '-' '/dev/vda2'
# stdin: <redacted secret>
'/usr/sbin/cryptsetup' 'open' '--key-file' '-' '/dev/vda2' '${MAPPER_NAME}'
'/usr/sbin/mkfs.btrfs' '-f' '-L' 'borea_root' '/dev/mapper/${MAPPER_NAME}'
'mount' '-t' 'btrfs' '-o' 'subvolid=5' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@'
'/usr/bin/chmod' '0755' '/run/borea/target/@'
'/usr/bin/chown' '0:0' '/run/borea/target/@'
'/usr/bin/mkdir' '--parents' '--mode=0755' '/run/borea/target/@'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@/home'
'/usr/bin/chmod' '0755' '/run/borea/target/@/home'
'/usr/bin/chown' '0:0' '/run/borea/target/@/home'
'/usr/bin/mkdir' '--parents' '--mode=0755' '/run/borea/target/@'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@/root'
'/usr/bin/chmod' '0700' '/run/borea/target/@/root'
'/usr/bin/chown' '0:0' '/run/borea/target/@/root'
'/usr/bin/mkdir' '--parents' '--mode=0755' '/run/borea/target/@'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@/opt'
'/usr/bin/chmod' '0755' '/run/borea/target/@/opt'
'/usr/bin/chown' '0:0' '/run/borea/target/@/opt'
'/usr/bin/mkdir' '--parents' '--mode=0755' '/run/borea/target/@'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@/srv'
'/usr/bin/chmod' '0755' '/run/borea/target/@/srv'
'/usr/bin/chown' '0:0' '/run/borea/target/@/srv'
'/usr/bin/mkdir' '--parents' '--mode=0755' '/run/borea/target/@/usr'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@/usr/local'
'/usr/bin/chmod' '0755' '/run/borea/target/@/usr/local'
'/usr/bin/chown' '0:0' '/run/borea/target/@/usr/local'
'/usr/bin/mkdir' '--parents' '--mode=0755' '/run/borea/target/@'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@/var'
'/usr/bin/chmod' '0755' '/run/borea/target/@/var'
'/usr/bin/chown' '0:0' '/run/borea/target/@/var'
'/usr/bin/mkdir' '--parents' '--mode=0755' '/run/borea/target/@'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@/.snapshots'
'/usr/bin/chmod' '0700' '/run/borea/target/@/.snapshots'
'/usr/bin/chown' '0:0' '/run/borea/target/@/.snapshots'
'/usr/bin/btrfs' 'inspect-internal' 'rootid' '/run/borea/target/@'
'/usr/bin/btrfs' 'subvolume' 'set-default' '${ROOT_SUBVOL_ID}' '/run/borea/target'
'umount' '/run/borea/target'
'mount' '-t' 'btrfs' '-o' 'subvol=@,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target'
'mount' '-t' 'btrfs' '-o' 'subvol=@/home,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/home'
'mount' '-t' 'btrfs' '-o' 'subvol=@/root,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/root'
'mount' '-t' 'btrfs' '-o' 'subvol=@/opt,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/opt'
'mount' '-t' 'btrfs' '-o' 'subvol=@/srv,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/srv'
'mount' '-t' 'btrfs' '-o' 'subvol=@/usr/local,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/usr/local'
'mount' '-t' 'btrfs' '-o' 'subvol=@/var,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/var'
'mount' '-t' 'btrfs' '-o' 'subvol=@/.snapshots,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/.snapshots'
'mount' '-t' 'vfat' '-o' 'umask=0077' '/dev/vda1' '/run/borea/target/boot'
'/usr/sbin/blkid' '-s' 'UUID' '-o' 'value' '/dev/vda2'
'/usr/sbin/blkid' '-s' 'UUID' '-o' 'value' '/dev/mapper/${MAPPER_NAME}'

# Installing openSUSE.
# write target:/etc/kernel/cmdline mode 644
# rd.luks.name=${LUKS_UUID}=borea-root root=/dev/mapper/borea-root rootflags=subvol=@
#
# Copy native openSUSE repository definitions from the authenticated tool root into the target.
chroot '/run/borea/tools/opensuse' '/usr/bin/zypper' '--root' '/mnt' '--gpg-auto-import-keys' '--non-interactive' 'install' '--auto-agree-with-licenses' '--type' 'pattern' 'minimal_base'
chroot '/run/borea/tools/opensuse' '/usr/bin/zypper' '--root' '/mnt' '--non-interactive' 'install' '--auto-agree-with-licenses' 'NetworkManager' 'bluez' 'btrfsprogs' 'cryptsetup' 'cups' 'dracut' 'firewalld' 'glibc-locale' 'kbd' 'kernel-default' 'kernel-firmware-all' 'nano' 'patterns-kde-kde_plasma' 'pipewire' 'sdbootutil' 'sddm' 'shadow' 'shim' 'snapper' 'sudo' 'systemd' 'systemd-boot' 'ucode-amd' 'util-linux-systemd' 'zram-generator'
'mount' '-t' 'proc' '-o' 'nosuid,nodev,noexec' 'proc' '/run/borea/target/proc'
'mount' '-o' 'rbind,ro' '/sys' '/run/borea/target/sys'
'mount' '-o' 'rbind' '/dev' '/run/borea/target/dev'
'mount' '-t' 'tmpfs' '-o' 'mode=0755,nosuid,nodev' 'tmpfs' '/run/borea/target/run'
# Copy only the active NetworkManager profile identified by its UUID, root-owned mode 0600; preserve credentials verbatim. Copy DNS configuration for the native chroot.

# Setting up drivers and repositories.
chroot /run/borea/target '/usr/bin/zypper' '--non-interactive' 'install' 'openSUSE-repos-NVIDIA'
chroot /run/borea/target '/usr/bin/zypper' '--non-interactive' 'install' '--auto-agree-with-licenses' 'nvidia-compute-utils-G06' 'nvidia-driver-G06-kmp-meta' 'nvidia-gl-G06' 'nvidia-video-G06'
# write target:/etc/modprobe.d/borea.conf mode 644
# options nvidia NVreg_PreserveVideoMemoryAllocations=1
#

# Configuring your system and account.
# Install the static Rust first-boot/updater helper, an empty root-owned queue, and its systemd units.
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'borea-firstboot.service'
# write target:/etc/hostname mode 644
# borea-test
#
# write target:/etc/hosts mode 644
# 127.0.0.1 localhost
# 127.0.1.1 borea-test
# ::1 localhost ip6-localhost ip6-loopback
#
# write target:/etc/locale.conf mode 644
# LANG=en_GB.UTF-8
#
# write target:/etc/vconsole.conf mode 644
# KEYMAP=uk
#
# write target:/etc/X11/xorg.conf.d/00-keyboard.conf mode 644
# Section "InputClass"
#  Identifier "borea-keyboard"
#  MatchIsKeyboard "on"
#  Option "XkbLayout" "gb"
#  Option "XkbVariant" ""
# EndSection
#
chroot /run/borea/target '/usr/bin/ln' '-sf' '/usr/share/zoneinfo/Europe/London' '/etc/localtime'
# write target:/etc/systemd/zram-generator.conf mode 644
# [zram0]
# zram-size = min(ram / 2, 8192)
# compression-algorithm = zstd
#
'/usr/sbin/blkid' '-s' 'UUID' '-o' 'value' '/dev/vda1'
# Write fstab (and crypttab when encrypting) from discovered filesystem and LUKS UUIDs and the reviewed btrfs layout.
# write target:/etc/snapper/configs/root mode 640
# SUBVOLUME="/"
# FSTYPE="btrfs"
# TIMELINE_CREATE="yes"
# TIMELINE_CLEANUP="yes"
# NUMBER_CLEANUP="yes"
# NUMBER_LIMIT="50"
#
# write target:/etc/sysconfig/snapper mode 644
# SNAPPER_CONFIGS="root"
#
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'snapper-timeline.timer' 'snapper-cleanup.timer'
chroot /run/borea/target '/usr/sbin/groupadd' '--system' '--force' 'wheel'
chroot /run/borea/target '/usr/sbin/useradd' '--create-home' '--user-group' '--shell' '/bin/bash' '--groups' 'wheel' '--comment' 'Test User' 'tester'
# stdin: <redacted secret>
chroot /run/borea/target '/usr/sbin/chpasswd'
# stdin: <redacted secret>
chroot /run/borea/target '/usr/sbin/chpasswd'
# write target:/etc/sudoers.d/10-borea mode 440
# %wheel ALL=(ALL:ALL) ALL
#
chroot /run/borea/target '/usr/sbin/visudo' '-cf' '/etc/sudoers.d/10-borea'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'NetworkManager.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'bluetooth.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'cups.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'firewalld.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'sddm.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'fstrim.timer'
chroot /run/borea/target '/usr/bin/systemctl' 'set-default' 'graphical.target'

# Adding your apps.

# Finishing up.
# write target:/etc/kernel/cmdline mode 644
# rd.luks.name=${LUKS_UUID}=borea-root root=/dev/mapper/borea-root rootflags=subvol=@
#
# write target:/etc/dracut.conf.d/90-borea.conf mode 644
# hostonly_cmdline="no"
#
chroot /run/borea/target '/usr/bin/dracut' '--regenerate-all' '--force'
# write target:/etc/sysconfig/bootloader mode 644
# LOADER_TYPE="systemd-boot"
# SECURE_BOOT="no"
# UPDATE_NVRAM="yes"
#
chroot /run/borea/target '/usr/bin/sdbootutil' '--esp-path' '/boot' '--arch' 'x64' 'install'
chroot /run/borea/target '/usr/bin/sdbootutil' '--esp-path' '/boot' '--arch' 'x64' 'add-all-kernels'
# Unmount the temporary hashed transport CA directory and regenerate the installed system's native certificate cache. Session proxy trust is never persisted.
# Restore the target's native DNS file/symlink and refresh the selected Wi-Fi profile before unmounting.
'/usr/bin/sync'

# Cleanup: stop child processes only in the job-owned private namespace,
# unmount owned trees deepest first and close only this job's encrypted mapping.
# Restore saved EFI/DNS configuration and preserve the redacted install log.
