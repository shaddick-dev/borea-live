#!/usr/bin/env bash
# REVIEW ONLY: secrets and runtime identifiers are placeholders.
# Execution is exclusively through the approved privileged service.
exit 64

set -euo pipefail

# Verifying the native installation tools.
# HTTPS GET https://example.org/arch.tar.xz -> /run/borea/tools/arch.tar; verify SHA256 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa before unpacking.
# HTTPS GET https://example.org/image.sig -> /run/borea/tools/arch.sig
'/usr/bin/gpgv' '--keyring' '/usr/share/keyrings/borea/arch.gpg' '/run/borea/tools/arch.sig' '/run/borea/tools/arch.tar'
'/usr/bin/tar' '-tf' '/run/borea/tools/arch.tar'
# Archive format is detected only after authentication; the matching branch runs:
'/usr/bin/tar' '--extract' '--file' '/run/borea/tools/arch.tar' '--directory' '/run/borea/tools/arch' '--strip-components=1'
'/usr/bin/skopeo' 'copy' '<docker-archive-or-oci-archive>:/run/borea/tools/arch.tar' 'oci:/run/borea/tools/arch.oci:borea'
'/usr/bin/umoci' 'unpack' '--image' '/run/borea/tools/arch.oci:borea' '/run/borea/tools/arch.bundle'
# For container archives move the verified bundle/rootfs into the tool root. Mount proc, sys and dev there; native tooling must exist. Temporary session CA trust is copied into /run.
# Check live utilities and the static x86-64 target helper before disk changes.
# Write the HTTPS Arch mirrorlist and enable multilib only when the reviewed selection requires it.
chroot '/run/borea/tools/arch' '/usr/bin/pacman-key' '--init'
chroot '/run/borea/tools/arch' '/usr/bin/pacman-key' '--populate' 'archlinux'
chroot '/run/borea/tools/arch' '/usr/bin/pacman' '-Sy' '--noconfirm'
chroot '/run/borea/tools/arch' '/usr/bin/pacman' '-Sp' '--noconfirm' 'base' 'btrfs-progs' 'chrony' 'cockpit' 'cryptsetup' 'dosfstools' 'fail2ban' 'firewalld' 'intel-ucode' 'kbd' 'linux' 'linux-firmware' 'mkinitcpio' 'nano' 'networkmanager' 'nvidia-open' 'nvidia-settings' 'nvidia-utils' 'openssh' 'rsync' 'sbctl' 'sbsigntools' 'sudo' 'systemd' 'systemd-ukify' 'tmux' 'zram-generator' 'lib32-nvidia-utils'

# Making space.
'/usr/sbin/ntfsresize' '--no-progress-bar' '--force' '--size' '48318382080' '/dev/vda3'
# public stdin: size=94371840
'/usr/sbin/sfdisk' '--no-reread' '--no-tell-kernel' '--wipe=never' '--wipe-partitions=never' '-N' '3' '/dev/vda'
'/usr/sbin/sgdisk' '--new=4:100663296:195035135' '--typecode=4:8309' '--change-name=4:borea_root' '/dev/vda'
'/usr/sbin/partprobe' '/dev/vda'
'/usr/bin/udevadm' 'settle'

# Locking your space.
# stdin: <redacted secret>
'/usr/sbin/cryptsetup' 'luksFormat' '--batch-mode' '--type' 'luks2' '--pbkdf' 'argon2id' '--cipher' 'aes-xts-plain64' '--key-size' '512' '--label' 'borea_root' '--key-file' '-' '/dev/vda4'
# stdin: <redacted secret>
'/usr/sbin/cryptsetup' 'open' '--key-file' '-' '/dev/vda4' '${MAPPER_NAME}'
'/usr/sbin/mkfs.btrfs' '-f' '-L' 'borea_root' '/dev/mapper/${MAPPER_NAME}'
'mount' '-t' 'btrfs' '-o' 'subvolid=5' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@'
'/usr/bin/chmod' '0755' '/run/borea/target/@'
'/usr/bin/chown' '0:0' '/run/borea/target/@'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@home'
'/usr/bin/chmod' '0755' '/run/borea/target/@home'
'/usr/bin/chown' '0:0' '/run/borea/target/@home'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@var_log'
'/usr/bin/chmod' '0755' '/run/borea/target/@var_log'
'/usr/bin/chown' '0:0' '/run/borea/target/@var_log'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@cache'
'/usr/bin/chmod' '0755' '/run/borea/target/@cache'
'/usr/bin/chown' '0:0' '/run/borea/target/@cache'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@snapshots'
'/usr/bin/chmod' '0700' '/run/borea/target/@snapshots'
'/usr/bin/chown' '0:0' '/run/borea/target/@snapshots'
'umount' '/run/borea/target'
'mount' '-t' 'btrfs' '-o' 'subvol=@,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target'
'mount' '-t' 'btrfs' '-o' 'subvol=@home,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/home'
'mount' '-t' 'btrfs' '-o' 'subvol=@var_log,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/var/log'
'mount' '-t' 'btrfs' '-o' 'subvol=@cache,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/var/cache'
'mount' '-t' 'btrfs' '-o' 'subvol=@snapshots,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/.snapshots'
'mount' '-t' 'vfat' '-o' 'umask=0077' '/dev/vda1' '/run/borea/target/boot'
# Keep existing EFI/BOOT files in bounded RAM before native package/bootloader actions; reject symlinks and oversized trees.
'/usr/sbin/blkid' '-s' 'UUID' '-o' 'value' '/dev/vda4'
'/usr/sbin/blkid' '-s' 'UUID' '-o' 'value' '/dev/mapper/${MAPPER_NAME}'

# Installing Arch Linux.
# write target:/etc/kernel/cmdline mode 644
# rd.luks.name=${LUKS_UUID}=borea-root root=/dev/mapper/borea-root rootflags=subvol=@
#
chroot '/run/borea/tools/arch' '/usr/bin/pacstrap' '-K' '/mnt' 'base' 'btrfs-progs' 'chrony' 'cockpit' 'cryptsetup' 'dosfstools' 'fail2ban' 'firewalld' 'intel-ucode' 'kbd' 'linux' 'linux-firmware' 'mkinitcpio' 'nano' 'networkmanager' 'nvidia-open' 'nvidia-settings' 'nvidia-utils' 'openssh' 'rsync' 'sbctl' 'sbsigntools' 'sudo' 'systemd' 'systemd-ukify' 'tmux' 'zram-generator'
'mount' '-t' 'proc' '-o' 'nosuid,nodev,noexec' 'proc' '/run/borea/target/proc'
'mount' '-o' 'rbind,ro' '/sys' '/run/borea/target/sys'
'mount' '-o' 'rbind' '/dev' '/run/borea/target/dev'
'mount' '-t' 'tmpfs' '-o' 'mode=0755,nosuid,nodev' 'tmpfs' '/run/borea/target/run'
# Copy only the active NetworkManager profile identified by its UUID, root-owned mode 0600; preserve credentials verbatim. Copy DNS configuration for the native chroot.

# Setting up drivers and repositories.
# write target:/etc/pacman.conf mode 644
# [options]
# Architecture = auto
# CheckSpace
# SigLevel = Required DatabaseOptional
# LocalFileSigLevel = Optional
# [core]
# Include = /etc/pacman.d/mirrorlist
# [extra]
# Include = /etc/pacman.d/mirrorlist
# [multilib]
# Include = /etc/pacman.d/mirrorlist
#
chroot /run/borea/target '/usr/bin/pacman' '-S' '--needed' '--noconfirm' 'lib32-nvidia-utils'
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
# write target:/etc/locale.gen mode 644
# en_GB.UTF-8 UTF-8
#
chroot /run/borea/target '/usr/bin/locale-gen'
# write target:/etc/ssh/sshd_config.d/90-borea-root.conf mode 644
# PermitRootLogin no
#
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
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'chronyd.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'cockpit.socket'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'firewalld.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'sshd.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'fstrim.timer'
chroot /run/borea/target '/usr/bin/systemctl' 'set-default' 'multi-user.target'
chroot /run/borea/target '/usr/bin/firewall-offline-cmd' '--add-service=ssh'
chroot /run/borea/target '/usr/bin/firewall-offline-cmd' '--add-service=cockpit'

# Adding your apps.
# write target:/var/lib/borea/apps.json mode 644
# {}

# Finishing up.
# write target:/etc/kernel/cmdline mode 644
# rd.luks.name=${LUKS_UUID}=borea-root root=/dev/mapper/borea-root rootflags=subvol=@
#
# write target:/etc/mkinitcpio.conf mode 644
# MODULES=()
# BINARIES=()
# FILES=()
# HOOKS=(base systemd autodetect microcode modconf kms keyboard sd-vconsole block sd-encrypt filesystems fsck)
#
chroot /run/borea/target '/usr/bin/mkinitcpio' '-P'
chroot /run/borea/target '/usr/bin/bootctl' '--esp-path=/boot' 'install'
# write target:/boot/loader/loader.conf mode 644
# default arch.conf
# timeout 4
# editor no
#
# write target:/boot/loader/entries/arch.conf mode 644
# title Arch Linux
# linux /vmlinuz-linux
# initrd /initramfs-linux.img
# options rd.luks.name=${LUKS_UUID}=borea-root root=/dev/mapper/borea-root rootflags=subvol=@
#
chroot /run/borea/target '/usr/bin/pacman' '-S' '--needed' '--noconfirm' 'sbctl' 'sbsigntools'
chroot /run/borea/target '/usr/bin/sbctl' 'create-keys'
# write target:/etc/mkinitcpio.d/linux.preset mode 644
# ALL_config="/etc/mkinitcpio.conf"
# ALL_kver="/boot/vmlinuz-linux"
# PRESETS=('default')
# default_uki="/boot/EFI/Linux/arch-linux.efi"
#
chroot /run/borea/target '/usr/bin/mkdir' '-p' '/boot/EFI/Linux'
chroot /run/borea/target '/usr/bin/mkinitcpio' '-P'
chroot /run/borea/target '/usr/bin/sbctl' 'sign' '--save' '/boot/EFI/systemd/systemd-bootx64.efi'
chroot /run/borea/target '/usr/bin/sbctl' 'sign' '--save' '/boot/EFI/Linux/arch-linux.efi'
# Only when no pre-existing fallback BOOTX64.EFI was saved: sign and track the newly installed Arch fallback. Existing fallback binaries are never signed or tracked.
chroot '/run/borea/target' '/usr/bin/sbctl' 'sign' '--save' '/boot/EFI/BOOT/BOOTX64.EFI'
chroot /run/borea/target '/usr/bin/sbctl' 'enroll-keys' '-m'
# write target:/boot/loader/entries/arch.conf mode 644
# title Arch Linux
# efi /EFI/Linux/arch-linux.efi
#
# write target:/etc/pacman.d/hooks/zz-borea-secureboot.hook mode 644
# [Trigger]
# Operation = Install
# Operation = Upgrade
# Type = Package
# Target = linux
# Target = systemd
# [Action]
# Description = Renew Borea Secure Boot signatures
# When = PostTransaction
# Exec = /usr/bin/sbctl sign-all
#
# Restore every existing EFI/BOOT file verbatim before unmounting, also on failure.
# Restore the target's native DNS file/symlink and refresh the selected Wi-Fi profile before unmounting.
'/usr/bin/sync'

# Cleanup: stop child processes only in the job-owned private namespace,
# unmount owned trees deepest first and close only this job's encrypted mapping.
# Restore saved EFI/DNS configuration and preserve the redacted install log.
