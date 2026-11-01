#!/usr/bin/env bash
# REVIEW ONLY: secrets and runtime identifiers are placeholders.
# Execution is exclusively through the approved privileged service.
exit 64

set -euo pipefail

# Verifying the native installation tools.
# HTTPS GET https://example.org/fedora.tar.xz -> /run/borea/tools/fedora.tar; verify SHA256 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa before unpacking.
# HTTPS GET https://example.org/image.sig -> /run/borea/tools/fedora.sig
'/usr/bin/gpgv' '--keyring' '/usr/share/keyrings/borea/fedora.gpg' '/run/borea/tools/fedora.sig' '/run/borea/tools/fedora.tar'
'/usr/bin/tar' '-tf' '/run/borea/tools/fedora.tar'
# Archive format is detected only after authentication; the matching branch runs:
'/usr/bin/tar' '--extract' '--file' '/run/borea/tools/fedora.tar' '--directory' '/run/borea/tools/fedora' '--strip-components=0'
'/usr/bin/skopeo' 'copy' '<docker-archive-or-oci-archive>:/run/borea/tools/fedora.tar' 'oci:/run/borea/tools/fedora.oci:borea'
'/usr/bin/umoci' 'unpack' '--image' '/run/borea/tools/fedora.oci:borea' '/run/borea/tools/fedora.bundle'
# For container archives move the verified bundle/rootfs into the tool root. Mount proc, sys and dev there; native tooling must exist. Temporary session CA trust is copied into /run.
# Check live utilities and the static x86-64 target helper before disk changes.
chroot '/run/borea/tools/fedora' '/usr/bin/dnf' '--setopt=localpkg_gpgcheck=1' 'install' '-y' 'https://download1.rpmfusion.org/nonfree/fedora/rpmfusion-nonfree-release-43.noarch.rpm'
chroot '/run/borea/tools/fedora' '/usr/bin/dnf' 'makecache' '--refresh'
chroot '/run/borea/tools/fedora' '/usr/bin/dnf' 'repoquery' '--available' '--queryformat=%{name}\n' 'NetworkManager' 'alsa-sof-firmware' 'btrfs-progs' 'chrony' 'cockpit' 'cryptsetup' 'dracut' 'efibootmgr' 'fail2ban' 'firewalld' 'glibc-langpack-en' 'grub2-efi-x64' 'grub2-tools' 'grubby' 'kbd' 'kernel' 'kernel-core' 'kernel-modules' 'libva-intel-media-driver' 'linux-firmware' 'mesa-dri-drivers' 'mesa-vulkan-drivers' 'microcode_ctl' 'mokutil' 'nano' 'openssh-server' 'os-prober' 'policycoreutils' 'rsync' 'selinux-policy-targeted' 'shim-x64' 'sudo' 'switcheroo-control' 'systemd' 'tmux' 'tuned-ppd' 'zram-generator' 'akmod-nvidia' 'xorg-x11-drv-nvidia-cuda' 'xorg-x11-drv-nvidia-power'

# Clearing the drive.
'/usr/sbin/sgdisk' '--zap-all' '/dev/vda'
'/usr/sbin/sgdisk' '--new=1:2048:2099199' '--typecode=1:ef00' '--change-name=1:EFI System' '/dev/vda'
'/usr/sbin/sgdisk' '--new=2:2099200:4196351' '--typecode=2:8300' '--change-name=2:Linux boot' '/dev/vda'
'/usr/sbin/sgdisk' '--new=3:4196352:209715166' '--typecode=3:8309' '--change-name=3:borea_root' '/dev/vda'
'/usr/sbin/partprobe' '/dev/vda'
'/usr/bin/udevadm' 'settle'
'/usr/sbin/mkfs.fat' '-F' '32' '/dev/vda1'
'/usr/sbin/mkfs.ext4' '-F' '/dev/vda2'

# Locking your space.
# stdin: <redacted secret>
'/usr/sbin/cryptsetup' 'luksFormat' '--batch-mode' '--type' 'luks2' '--pbkdf' 'argon2id' '--cipher' 'aes-xts-plain64' '--key-size' '512' '--label' 'borea_root' '--key-file' '-' '/dev/vda3'
# stdin: <redacted secret>
'/usr/sbin/cryptsetup' 'open' '--key-file' '-' '/dev/vda3' '${MAPPER_NAME}'
'/usr/sbin/mkfs.btrfs' '-f' '-L' 'borea_root' '/dev/mapper/${MAPPER_NAME}'
'mount' '-t' 'btrfs' '-o' 'subvolid=5' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/root'
'/usr/bin/chmod' '0755' '/run/borea/target/root'
'/usr/bin/chown' '0:0' '/run/borea/target/root'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/home'
'/usr/bin/chmod' '0755' '/run/borea/target/home'
'/usr/bin/chown' '0:0' '/run/borea/target/home'
'umount' '/run/borea/target'
'mount' '-t' 'btrfs' '-o' 'subvol=root,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target'
'mount' '-t' 'btrfs' '-o' 'subvol=home,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/home'
'mount' '-t' 'ext4' '-o' 'defaults' '/dev/vda2' '/run/borea/target/boot'
'mount' '-t' 'vfat' '-o' 'umask=0077' '/dev/vda1' '/run/borea/target/boot/efi'
'/usr/sbin/blkid' '-s' 'UUID' '-o' 'value' '/dev/vda3'
'/usr/sbin/blkid' '-s' 'UUID' '-o' 'value' '/dev/mapper/${MAPPER_NAME}'

# Installing Fedora.
# write target:/etc/kernel/install.conf mode 644
# layout=bls
# BOOT_ROOT=/boot
#
# write target:/etc/kernel/cmdline mode 644
# rd.luks.name=${LUKS_UUID}=borea-root root=/dev/mapper/borea-root rootflags=subvol=root
#
chroot '/run/borea/tools/fedora' '/usr/bin/dnf' '--installroot=/mnt' '--releasever=43' '--use-host-config' 'install' '-y' '@core' 'NetworkManager' 'alsa-sof-firmware' 'btrfs-progs' 'chrony' 'cockpit' 'cryptsetup' 'dracut' 'efibootmgr' 'fail2ban' 'firewalld' 'glibc-langpack-en' 'grub2-efi-x64' 'grub2-tools' 'grubby' 'kbd' 'kernel' 'kernel-core' 'kernel-modules' 'libva-intel-media-driver' 'linux-firmware' 'mesa-dri-drivers' 'mesa-vulkan-drivers' 'microcode_ctl' 'mokutil' 'nano' 'openssh-server' 'os-prober' 'policycoreutils' 'rsync' 'selinux-policy-targeted' 'shim-x64' 'sudo' 'switcheroo-control' 'systemd' 'tmux' 'tuned-ppd' 'zram-generator'
'mount' '-t' 'proc' '-o' 'nosuid,nodev,noexec' 'proc' '/run/borea/target/proc'
'mount' '-o' 'rbind,ro' '/sys' '/run/borea/target/sys'
'mount' '-o' 'rbind' '/dev' '/run/borea/target/dev'
'mount' '-t' 'tmpfs' '-o' 'mode=0755,nosuid,nodev' 'tmpfs' '/run/borea/target/run'
# Copy only the active NetworkManager profile identified by its UUID, root-owned mode 0600; preserve credentials verbatim. Copy DNS configuration for the native chroot.

# Setting up drivers and repositories.
chroot /run/borea/target '/usr/bin/dnf' '--setopt=localpkg_gpgcheck=1' 'install' '-y' 'https://download1.rpmfusion.org/nonfree/fedora/rpmfusion-nonfree-release-43.noarch.rpm'
chroot /run/borea/target '/usr/bin/dnf' 'install' '-y' 'akmod-nvidia' 'xorg-x11-drv-nvidia-cuda' 'xorg-x11-drv-nvidia-power'
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
'/usr/sbin/blkid' '-s' 'UUID' '-o' 'value' '/dev/vda2'
# Write fstab (and crypttab when encrypting) from discovered filesystem and LUKS UUIDs and the reviewed btrfs layout.
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
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'nvidia-hibernate.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'nvidia-powerd.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'nvidia-resume.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'nvidia-suspend.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'sshd.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'switcheroo-control.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'tuned-ppd.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'tuned.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'fstrim.timer'
chroot /run/borea/target '/usr/bin/systemctl' 'set-default' 'multi-user.target'
chroot /run/borea/target '/usr/bin/firewall-offline-cmd' '--add-service=ssh'
chroot /run/borea/target '/usr/bin/firewall-offline-cmd' '--add-service=cockpit'

# Adding your apps.
# write target:/var/lib/borea/apps.json mode 644
# {}

# Finishing up.
# write target:/etc/kernel/cmdline mode 644
# rd.luks.name=${LUKS_UUID}=borea-root root=/dev/mapper/borea-root rootflags=subvol=root
#
# write target:/etc/default/grub mode 644
# GRUB_DEFAULT=saved
# GRUB_TIMEOUT=4
# GRUB_ENABLE_BLSCFG=true
# GRUB_DISABLE_OS_PROBER=false
# GRUB_CMDLINE_LINUX="rd.luks.name=${LUKS_UUID}=borea-root root=/dev/mapper/borea-root rootflags=subvol=root "
#
chroot /run/borea/target '/usr/bin/mkdir' '-p' '/boot/loader/entries' '/boot/grub2'
# write target:/etc/kernel/install.conf mode 644
# layout=bls
# BOOT_ROOT=/boot
#
chroot /run/borea/target '/usr/bin/kernel-install' 'add-all'
chroot /run/borea/target '/usr/sbin/grubby' '--update-kernel=ALL' '--remove-args=root rootflags rd.luks.uuid rd.luks.name' '--args=rd.luks.name=${LUKS_UUID}=borea-root root=/dev/mapper/borea-root rootflags=subvol=root '
chroot /run/borea/target '/usr/bin/grub2-editenv' '-' 'set' 'kernelopts=rd.luks.name=${LUKS_UUID}=borea-root root=/dev/mapper/borea-root rootflags=subvol=root '
chroot /run/borea/target '/usr/bin/dracut' '--regenerate-all' '--force'
chroot /run/borea/target '/usr/sbin/grub2-mkconfig' '-o' '/boot/grub2/grub.cfg'
# write target:/boot/efi/EFI/fedora/grub.cfg mode 644
# search --no-floppy --fs-uuid --set=root ${BOOT_UUID}
# set prefix=($root)/grub2
# configfile $prefix/grub.cfg
#
# write target:/.autorelabel mode 600
# -F
#
'/usr/sbin/efibootmgr' '--create' '--disk' '/dev/vda' '--part' '1' '--label' 'Fedora' '--loader' '\EFI\fedora\shimx64.efi'
chroot /run/borea/target '/usr/bin/find' '/usr/lib/modules' '-mindepth' '1' '-maxdepth' '1' '-printf' '%f
'
chroot /run/borea/target '/usr/sbin/akmods' '--force' '--kernels' '${KERNEL_VERSION}'
chroot /run/borea/target '/usr/bin/dracut' '--regenerate-all' '--force'
chroot /run/borea/target '/usr/bin/dnf' 'install' '-y' 'mokutil'
# stdin: <redacted secret>, twice. Enrollment is confirmed in firmware on first restart.
chroot '/run/borea/target' '/usr/bin/mokutil' '--import' '/etc/pki/akmods/certs/public_key.der'
# Restore the target's native DNS file/symlink and refresh the selected Wi-Fi profile before unmounting.
chroot /run/borea/target '/usr/sbin/setfiles' '-F' '-m' '-e' '/dev' '-e' '/proc' '-e' '/sys' '-e' '/run' '-e' '/boot/efi' '/etc/selinux/targeted/contexts/files/file_contexts' '/'
'/usr/bin/sync'

# Cleanup: stop child processes only in the job-owned private namespace,
# unmount owned trees deepest first and close only this job's encrypted mapping.
# Restore saved EFI/DNS configuration and preserve the redacted install log.
