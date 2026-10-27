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
	-p borea-installd --bin borea-installd \
	-p borea-cli --bin borea-cli \
	-p borea-init --bin borea-init

# borea-build wipes this package before every ISO build so it picks up new
# sources, but the compiled Rust lives outside the package directory, so the
# ~400 unchanged dependency crates are reused instead of compiled again.
# Delete $(BASE_DIR)/borea-cargo to force a clean Rust build.
BOREA_CARGO_TARGET = $(BASE_DIR)/borea-cargo
# Thin link-time optimisation: much faster to link than the workspace's full
# LTO, for an installer whose speed is set by downloads and disks.
BOREA_CARGO_ENV = \
	CARGO_TARGET_DIR=$(BOREA_CARGO_TARGET)/main \
	CARGO_PROFILE_RELEASE_LTO=thin

define BOREA_INSTALL_TARGET_CMDS
	for bin in borea-live borea borea-flash borea-installd borea-cli; do \
		$(INSTALL) -D -m 0755 $(BOREA_CARGO_TARGET)/main/$(RUSTC_TARGET_NAME)/release/$$bin $(TARGET_DIR)/usr/bin/$$bin || exit 1; \
	done
	# The early boot's /init; the post-image hook copies it into the initramfs.
	$(INSTALL) -D -m 0755 $(BOREA_CARGO_TARGET)/main/$(RUSTC_TARGET_NAME)/release/borea-init \
		$(TARGET_DIR)/usr/lib/borea/borea-init
	# Pinned upstream signing keyrings (provenance: backend/dist/keys/PROVENANCE.md).
	for key in arch fedora opensuse; do \
		$(INSTALL) -D -m 0644 $(@D)/backend/dist/keys/$$key.gpg \
			$(TARGET_DIR)/usr/share/keyrings/borea/$$key.gpg || exit 1; \
	done
	$(INSTALL) -D -m 0755 $(BOREA_CARGO_TARGET)/static/$(RUSTC_TARGET_NAME)/release/borea-firstboot \
		$(TARGET_DIR)/usr/bin/borea-firstboot
endef

# borea-installd copies a statically linked first-boot helper into every target
# and refuses to install without one (crates/borea-exec runtime::static_helper).
# CI builds it for musl; here it is a static glibc build with the same toolchain.
define BOREA_BUILD_STATIC_HELPER
	cd $(@D) && $(TARGET_MAKE_ENV) $(PKG_CARGO_ENV) $(BOREA_CARGO_ENV) \
		RUSTFLAGS="-C target-feature=+crt-static" \
		$(HOST_DIR)/bin/cargo build --offline --locked --release \
		--target $(RUSTC_TARGET_NAME) --target-dir $(BOREA_CARGO_TARGET)/static \
		-p borea-exec --bin borea-firstboot
endef
BOREA_POST_BUILD_HOOKS += BOREA_BUILD_STATIC_HELPER

# The privileged installation service, enabled by a link in the rootfs overlay.
# The GUI and borea-cli reach it on /run/borea/installd.sock; peer credentials limit it to root and the live user.
define BOREA_INSTALL_INIT_SYSTEMD
	$(INSTALL) -D -m 0644 $(@D)/backend/dist/borea-installd.service \
		$(TARGET_DIR)/usr/lib/systemd/system/borea-installd.service
	$(INSTALL) -D -m 0644 $(@D)/backend/dist/borea.tmpfiles \
		$(TARGET_DIR)/usr/lib/tmpfiles.d/borea.conf
endef

$(eval $(cargo-package))
