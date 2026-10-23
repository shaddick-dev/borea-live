include $(sort $(wildcard $(BR2_EXTERNAL_BOREA_PATH)/package/*/*.mk))

# Buildroot builds polkit for ConsoleKit, which a systemd system never runs, so
# polkit could not see any session as local or active: the live user's polkit
# rule never applied, and logind refused Ctrl+Alt+F2. Track sessions through
# logind instead. (After changing this, `make polkit-dirclean` once.)
ifeq ($(BR2_PACKAGE_POLKIT)$(BR2_INIT_SYSTEMD),yy)
POLKIT_CONF_OPTS := $(filter-out -Dsession_tracking=ConsoleKit,$(POLKIT_CONF_OPTS)) \
	-Dsession_tracking=logind
polkit-depends: systemd
endif
