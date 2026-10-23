################################################################################
#
# limine
#
################################################################################

LIMINE_VERSION = 12.9.4
LIMINE_SOURCE = limine-$(LIMINE_VERSION).tar.xz
LIMINE_SITE = https://github.com/limine-bootloader/limine/releases/download/v$(LIMINE_VERSION)
LIMINE_LICENSE = BSD-2-Clause
LIMINE_LICENSE_FILES = COPYING
HOST_LIMINE_DEPENDENCIES = host-nasm host-mtools
HOST_LIMINE_CONF_OPTS = \
	--enable-bios \
	--enable-bios-cd \
	--enable-uefi-x86-64 \
	--enable-uefi-cd

$(eval $(host-autotools-package))
