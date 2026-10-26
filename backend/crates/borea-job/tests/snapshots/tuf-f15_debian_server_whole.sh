#!/usr/bin/env bash
# REVIEW ONLY: secrets and runtime identifiers are placeholders.
# Execution is exclusively through the approved privileged service.
exit 64

set -euo pipefail

# Verifying the native installation tools.
# Check live utilities and the static x86-64 target helper before disk changes.
'/usr/sbin/debootstrap' '--print-debs' '--variant=minbase' '--keyring=/usr/share/keyrings/debian-archive-keyring.gpg' '--components=main,non-free,non-free-firmware' '--include=btrfs-progs,chrony,cockpit,console-data,console-setup,cryptsetup-initramfs,efibootmgr,fail2ban,firewalld,grub-efi-amd64-signed,initramfs-tools,kbd,linux-headers-amd64,linux-image-amd64,locales,mesa-vulkan-drivers,mokutil,nano,network-manager,openssh-server,os-prober,power-profiles-daemon,rsync,shim-signed,sudo,switcheroo-control,systemd-sysv,systemd-zram-generator,tmux,intel-media-va-driver-non-free,nvidia-driver,nvidia-open-kernel-dkms,firmware-intel-graphics,firmware-linux,firmware-misc-nonfree,firmware-sof-signed,intel-microcode' 'trixie' '/run/borea/debian-preflight' 'https://deb.debian.org/debian'

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
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@'
'/usr/bin/chmod' '0755' '/run/borea/target/@'
'/usr/bin/chown' '0:0' '/run/borea/target/@'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@home'
'/usr/bin/chmod' '0755' '/run/borea/target/@home'
'/usr/bin/chown' '0:0' '/run/borea/target/@home'
'/usr/bin/btrfs' 'subvolume' 'create' '/run/borea/target/@var_log'
'/usr/bin/chmod' '0755' '/run/borea/target/@var_log'
'/usr/bin/chown' '0:0' '/run/borea/target/@var_log'
'umount' '/run/borea/target'
'mount' '-t' 'btrfs' '-o' 'subvol=@,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target'
'mount' '-t' 'btrfs' '-o' 'subvol=@home,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/home'
'mount' '-t' 'btrfs' '-o' 'subvol=@var_log,compress=zstd:1,noatime' '/dev/mapper/${MAPPER_NAME}' '/run/borea/target/var/log'
'mount' '-t' 'ext4' '-o' 'defaults' '/dev/vda2' '/run/borea/target/boot'
'mount' '-t' 'vfat' '-o' 'umask=0077' '/dev/vda1' '/run/borea/target/boot/efi'
'/usr/sbin/blkid' '-s' 'UUID' '-o' 'value' '/dev/vda3'
'/usr/sbin/blkid' '-s' 'UUID' '-o' 'value' '/dev/mapper/${MAPPER_NAME}'

# Installing Debian.
'/usr/sbin/debootstrap' '--variant=minbase' '--keyring=/usr/share/keyrings/debian-archive-keyring.gpg' '--include=ca-certificates' 'trixie' '/run/borea/target' 'https://deb.debian.org/debian'
'mount' '-t' 'proc' '-o' 'nosuid,nodev,noexec' 'proc' '/run/borea/target/proc'
'mount' '-t' 'bind' '-o' 'rbind,ro' '/sys' '/run/borea/target/sys'
'mount' '-t' 'bind' '-o' 'rbind' '/dev' '/run/borea/target/dev'
'mount' '-t' 'tmpfs' '-o' 'mode=0755,nosuid,nodev' 'tmpfs' '/run/borea/target/run'
# Copy only the active NetworkManager profile identified by its UUID, root-owned mode 0600; preserve credentials verbatim. Copy DNS configuration for the native chroot.

# Setting up drivers and repositories.
# write target:/etc/apt/sources.list.d/debian.sources mode 644
# Types: deb
# URIs: https://deb.debian.org/debian
# Suites: trixie trixie-updates
# Components: main non-free non-free-firmware
# Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
# 
# Types: deb
# URIs: https://security.debian.org/debian-security
# Suites: trixie-security
# Components: main non-free non-free-firmware
# Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
# 
chroot /run/borea/target '/usr/bin/apt-get' 'update'
chroot '/run/borea/target' '/usr/bin/dpkg-divert' '--add' '--rename' '--divert' '/usr/sbin/update-initramfs.borea-real' '/usr/sbin/update-initramfs'
# Temporarily install the static Rust initramfs guard; final boot configuration restores the native tool.
# write target:/etc/initramfs-tools/update-initramfs.conf mode 644
# update_initramfs=no
# backup_initramfs=no
# 
chroot /run/borea/target '/usr/bin/apt-get' '-o' 'Dpkg::Options::=--force-confold' 'install' '-y' '--no-install-recommends' 'btrfs-progs' 'chrony' 'cockpit' 'console-data' 'console-setup' 'cryptsetup-initramfs' 'efibootmgr' 'fail2ban' 'firewalld' 'grub-efi-amd64-signed' 'initramfs-tools' 'kbd' 'linux-headers-amd64' 'linux-image-amd64' 'locales' 'mesa-vulkan-drivers' 'mokutil' 'nano' 'network-manager' 'openssh-server' 'os-prober' 'power-profiles-daemon' 'rsync' 'shim-signed' 'sudo' 'switcheroo-control' 'systemd-sysv' 'systemd-zram-generator' 'tmux'
chroot /run/borea/target '/usr/bin/apt-get' 'install' '-y' '--no-install-recommends' 'intel-media-va-driver-non-free' 'nvidia-driver' 'nvidia-open-kernel-dkms'
chroot /run/borea/target '/usr/bin/apt-get' 'install' '-y' '--no-install-recommends' 'firmware-intel-graphics' 'firmware-linux' 'firmware-misc-nonfree' 'firmware-sof-signed' 'intel-microcode'
# write target:/etc/modprobe.d/borea.conf mode 644
# options nvidia NVreg_PreserveVideoMemoryAllocations=1
# 

# Configuring your system and account.
# Install the static Rust first-boot/updater helper, an empty root-owned queue, and its systemd units.
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'borea-firstboot.service'
# write target:/etc/hostname mode 644
# borea-test
# 
# write target:/etc/locale.conf mode 644
# LANG=en_GB.UTF-8
# 
# write target:/etc/default/locale mode 644
# LANG=en_GB.UTF-8
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
# Write fstab and crypttab from discovered filesystem/LUKS UUIDs and the reviewed btrfs layout.
# write target:/etc/default/keyboard mode 644
# XKBMODEL=pc105
# XKBLAYOUT=gb
# XKBVARIANT=
# XKBOPTIONS=
# BACKSPACE=guess
# 
chroot /run/borea/target '/usr/bin/setupcon' '--save-only'
# write target:/etc/locale.gen mode 644
# en_GB.UTF-8 UTF-8
# 
chroot /run/borea/target '/usr/bin/locale-gen' 
# write target:/etc/ssh/sshd_config.d/90-borea-root.conf mode 644
# PermitRootLogin no
# 
chroot /run/borea/target '/usr/sbin/useradd' '--create-home' '--user-group' '--shell' '/bin/bash' '--groups' 'sudo' '--comment' 'Test User' 'tester'
# stdin: <redacted secret>
chroot /run/borea/target '/usr/sbin/chpasswd' 
# stdin: <redacted secret>
chroot /run/borea/target '/usr/sbin/chpasswd' 
# write target:/etc/sudoers.d/10-borea mode 440
# %sudo ALL=(ALL:ALL) ALL
# 
chroot /run/borea/target '/usr/sbin/visudo' '-cf' '/etc/sudoers.d/10-borea'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'NetworkManager.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'chrony.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'cockpit.socket'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'firewalld.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'nvidia-hibernate.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'nvidia-powerd.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'nvidia-resume.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'nvidia-suspend.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'power-profiles-daemon.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'ssh.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'switcheroo-control.service'
chroot /run/borea/target '/usr/bin/systemctl' 'enable' 'fstrim.timer'
chroot /run/borea/target '/usr/bin/systemctl' 'set-default' 'multi-user.target'
chroot /run/borea/target '/usr/bin/firewall-offline-cmd' '--add-service=ssh'
chroot /run/borea/target '/usr/bin/firewall-offline-cmd' '--add-service=cockpit'

# Adding your apps.

# Finishing up.
# write target:/etc/kernel/cmdline mode 644
# rd.luks.name=${LUKS_UUID}=borea-root root=/dev/mapper/borea-root rootflags=subvol=@ nvidia-drm.modeset=1
# 
# write target:/etc/default/grub mode 644
# GRUB_DEFAULT=0
# GRUB_TIMEOUT=4
# GRUB_CMDLINE_LINUX="root=/dev/mapper/borea-root rootflags=subvol=@ nvidia-drm.modeset=1"
# GRUB_DISABLE_OS_PROBER=false
# 
chroot '/run/borea/target' '/usr/bin/dpkg-divert' '--remove' '--rename' '--divert' '/usr/sbin/update-initramfs.borea-real' '/usr/sbin/update-initramfs'
# Remove the temporary guard and restore the native initramfs tool before building boot images.
# write target:/etc/initramfs-tools/update-initramfs.conf mode 644
# update_initramfs=yes
# backup_initramfs=no
# 
chroot /run/borea/target '/usr/sbin/update-initramfs' '-c' '-k' 'all'
chroot /run/borea/target '/usr/bin/mkdir' '-p' '/boot/efi/EFI/debian' '/boot/efi/EFI/BOOT'
chroot /run/borea/target '/usr/bin/cp' '/usr/lib/shim/shimx64.efi.signed' '/boot/efi/EFI/debian/shimx64.efi'
chroot /run/borea/target '/usr/bin/cp' '/usr/lib/shim/mmx64.efi.signed' '/boot/efi/EFI/debian/mmx64.efi'
chroot /run/borea/target '/usr/bin/cp' '/usr/lib/grub/x86_64-efi-signed/grubx64.efi.signed' '/boot/efi/EFI/debian/grubx64.efi'
# write target:/boot/efi/EFI/debian/grub.cfg mode 644
# search --no-floppy --fs-uuid --set=root ${BOOT_UUID}
# set prefix=($root)/grub
# configfile $prefix/grub.cfg
# 
# Only when EFI/BOOT contains none of BOOTX64.EFI, grubx64.efi or grub.cfg: copy /boot/efi/EFI/debian/shimx64.efi, /boot/efi/EFI/debian/grubx64.efi and the UUID-based loader configuration. Preserve existing fallback files.
'/usr/sbin/efibootmgr' '--create' '--disk' '/dev/vda' '--part' '1' '--label' 'Debian' '--loader' '\EFI\debian\shimx64.efi'
chroot /run/borea/target '/usr/sbin/update-grub' 
chroot /run/borea/target '/usr/bin/apt-get' 'install' '-y' '--no-install-recommends' 'mokutil'
# stdin: <redacted secret>, twice. Enrollment is confirmed in firmware on first restart.
chroot '/run/borea/target' '/usr/bin/mokutil' '--import' '/var/lib/dkms/mok.pub'
# Restore the target's native DNS file/symlink and refresh the selected Wi-Fi profile before unmounting.
'/usr/bin/sync' 

# Cleanup: unmount owned paths in reverse order, stop private GPG agents,
# and close only this job's encrypted mapping. Preserve the redacted install log.
