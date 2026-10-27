include $(sort $(wildcard $(BR2_EXTERNAL_BOREA_PATH)/package/*/*.mk))

# Buildroot builds polkit for ConsoleKit, which a systemd system never runs, so
# polkit could not see any session as local or active: the live user's polkit
# rule never applied, and logind refused Ctrl+Alt+F2. Track sessions through
# logind instead, which needs systemd's libsystemd when polkit is built.
#
# Buildroot's BR2_PACKAGE_SYSTEMD_POLKIT would make systemd wait for polkit,
# the opposite order, so the two waited on each other and a fresh build
# configured polkit before libsystemd existed. That option stays off in the
# defconfig: systemd only looks polkit up at build time (meson: required
# false) and talks to it over D-Bus, so its polkit support is enabled here
# without that ordering.
ifeq ($(BR2_PACKAGE_POLKIT)$(BR2_INIT_SYSTEMD),yy)
POLKIT_CONF_OPTS := $(filter-out -Dsession_tracking=ConsoleKit,$(POLKIT_CONF_OPTS)) \
	-Dsession_tracking=logind
# Order polkit's configure after systemd, and (with per-package directories)
# give it systemd's files.
POLKIT_DEPENDENCIES += systemd
$(POLKIT_TARGET_CONFIGURE): | systemd
SYSTEMD_CONF_OPTS := $(filter-out -Dpolkit=disabled,$(SYSTEMD_CONF_OPTS)) \
	-Dpolkit=enabled
endif

# Every Intel Wi-Fi card. Buildroot's per-generation options leave out some of the
# most common laptop cards: the AX200 (cc), the AX201 in Alder and Raptor Lake
# laptops (so-hr), and the AX211 and BE200 families of Meteor Lake and later (ma,
# bz, sc). Without its firmware iwlwifi never creates the device, so NetworkManager
# has no Wi-Fi adapter to offer and the installer cannot get past Connect.
# Only the three newest firmware versions of each card are kept: iwlwifi takes the
# newest its kernel speaks, and the older ones would nearly double the image.
# Also Qualcomm's WCN7850 (ath12k, Wi-Fi 7), which has no Buildroot option.
ifeq ($(BR2_PACKAGE_LINUX_FIRMWARE),y)
LINUX_FIRMWARE_FILES += intel/iwlwifi/iwlwifi-*
LINUX_FIRMWARE_DIRS += ath12k/WCN7850

define BOREA_IWLWIFI_NEWEST_ONLY
	cd $(TARGET_DIR)/lib/firmware/intel/iwlwifi && \
	for card in $$(ls iwlwifi-*.ucode | sed -E 's/-c?[0-9]+\.ucode$$//' | sort -u); do \
		ls | grep -E "^$$card-c?[0-9]+\.ucode$$" | sort -V | head -n -3 | xargs -r rm -f; \
	done
	find -L $(TARGET_DIR)/lib/firmware -maxdepth 1 -type l -name 'iwlwifi-*' -delete
endef
LINUX_FIRMWARE_POST_INSTALL_TARGET_HOOKS += BOREA_IWLWIFI_NEWEST_ONLY
endif
