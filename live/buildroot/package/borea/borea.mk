################################################################################
#
# borea
#
################################################################################

# `borea-build iso` packs this workspace, with its dependencies vendored, into
# $(BOREA_SITE)/$(BOREA_SOURCE) before Buildroot runs, so the build is offline.
BOREA_VERSION = local
BOREA_SITE = $(BR2_EXTERNAL_BOREA_PATH)/../../target/live/src
BOREA_SITE_METHOD = file
BOREA_SOURCE = borea-$(BOREA_VERSION).tar.gz
BOREA_LICENSE = Apache-2.0 OR MIT (proposed)
BOREA_CARGO_BUILD_OPTS = \
	-p borea-gui --bin borea-live \
	-p borea-bootstrap --bin borea \
	-p borea-flash --bin borea-flash \
	-p borea-worker --bin borea-worker \
	-p borea-init --bin borea-init

define BOREA_INSTALL_TARGET_CMDS
	for bin in borea-live borea borea-flash borea-worker; do \
		$(INSTALL) -D -m 0755 $(@D)/target/$(RUSTC_TARGET_NAME)/release/$$bin $(TARGET_DIR)/usr/bin/$$bin || exit 1; \
	done
	# The early boot's /init; the post-image hook copies it into the initramfs.
	$(INSTALL) -D -m 0755 $(@D)/target/$(RUSTC_TARGET_NAME)/release/borea-init \
		$(TARGET_DIR)/usr/lib/borea/borea-init
endef

$(eval $(cargo-package))
