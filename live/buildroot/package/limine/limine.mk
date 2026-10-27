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
# Limine compiles its BIOS and UEFI stages with clang and links them with
# ld.lld (configure: CC_FOR_TARGET=clang, LD_FOR_TARGET=ld.lld, llvm-* tools).
# Build them here rather than relying on whatever the build machine has
# installed: without them configure stops ("clang invalid").
HOST_LIMINE_DEPENDENCIES = host-nasm host-mtools host-clang host-lld host-llvm
HOST_LIMINE_CONF_ENV = \
	CC_FOR_TARGET=$(HOST_DIR)/bin/clang \
	LD_FOR_TARGET=$(HOST_DIR)/bin/ld.lld \
	OBJCOPY_FOR_TARGET=$(HOST_DIR)/bin/llvm-objcopy \
	OBJDUMP_FOR_TARGET=$(HOST_DIR)/bin/llvm-objdump \
	READELF_FOR_TARGET=$(HOST_DIR)/bin/llvm-readelf
HOST_LIMINE_CONF_OPTS = \
	--enable-bios \
	--enable-bios-cd \
	--enable-uefi-x86-64 \
	--enable-uefi-cd

$(eval $(host-autotools-package))
