# Build the CPTV VLC plugin: Rust core (staticlib) + C VLC glue -> one shared module.
#
#   make                 build the plugin
#   make test            run Rust tests and a headless VLC smoke test
#
#   No root, VLC itself untouched:
#   make install-user    Linux: plugin in ~/.local, .cptv MIME type, and a "VLC (CPTV)"
#                        launcher set as the default app for .cptv files
#                        macOS: plugin in ~/Library/Application Support, a login
#                        LaunchAgent putting it on VLC's plugin path, and VLC set
#                        as the default app for .cptv files
#   Linux, system-wide / macOS:
#   make install         copy plugin into VLC's own plugin dir (sudo on Linux;
#                        on Linux also runs install-thumbnailer)
#   make associate       make VLC the default app for .cptv files
#   Linux file-manager thumbnails (GNOME only runs thumbnailers from /usr):
#   sudo make install-thumbnailer   thumbnailer + .thumbnailer entry into $(PREFIX)
#   sudo make uninstall-thumbnailer
#   On Linux, run plain `make` first: install targets never build under sudo.
#   macOS Quick Look (Finder thumbnails + spacebar preview):
#   make install-quicklook   build "CPTV Quick Look.app" into ~/Applications and register it
#   make uninstall-quicklook
#
# Variables you might override:
#   VLC_VERSION   VLC 3.0.x release whose headers to build against (default 3.0.23)
#   VLC_INCLUDE   use an existing header dir (e.g. /usr/include/vlc/plugins)
#   PLUGIN_DIR    where `make install` puts the plugin
#   VLC_APP       macOS VLC.app location (default /Applications/VLC.app)
#   VLC_ABI       plugin ABI tag, e.g. 3_0_0f or 3_0_0ft64 (default: detected
#                 from the installed libvlccore)
#   PREFIX        Linux: where `make install` puts the file-manager thumbnailer
#                 (default /usr/local)

VLC_VERSION ?= 3.0.23
UNAME_S     := $(shell uname -s)
CARGO       ?= cargo
CC          ?= cc

RUST_LIB    := target/release/libcptv_vlc.a
# Freedesktop thumbnailer (Linux file managers); also builds and tests on macOS.
THUMBNAILER := target/release/cptv-thumbnailer
PREFIX      ?= /usr/local
BUILD       := build

ifneq ($(shell pkg-config --exists vlc-plugin 2>/dev/null && echo yes),)
  VLC_INCLUDE ?= $(shell pkg-config --variable=includedir vlc-plugin)/vlc/plugins
endif
VLC_INCLUDE ?= third_party/vlc-$(VLC_VERSION)/include
HEADERS_STAMP := $(VLC_INCLUDE)/vlc_plugin.h

CFLAGS_ALL  := -std=gnu11 -O2 -fPIC -fvisibility=hidden -Wall -Wextra \
               -Wno-unused-parameter -Wno-sign-compare \
               -D_FILE_OFFSET_BITS=64 -D_REENTRANT -D_THREAD_SAFE \
               -I$(VLC_INCLUDE) -Ivlc $(CFLAGS)

ifeq ($(UNAME_S),Darwin)
  PLUGIN      := $(BUILD)/libcptv_plugin.dylib
  VLC_APP     ?= /Applications/VLC.app
  PLUGIN_DIR  ?= $(VLC_APP)/Contents/MacOS/plugins
  # Symbols from libvlccore are resolved by VLC at load time.
  LINK_FLAGS   = -dynamiclib -undefined dynamic_lookup \
                 -Wl,-exported_symbol,_vlc_entry__$(VLC_ABI) \
                 -Wl,-dead_strip -Wl,-x \
                 -framework CoreFoundation -lSystem -lc -lm -liconv
  VLC_BIN     ?= $(VLC_APP)/Contents/MacOS/VLC
  LIBVLCCORE  ?= $(firstword $(wildcard $(VLC_APP)/Contents/MacOS/lib/libvlccore*.dylib))
  CACHE_GEN   :=
  LINUX_EXTRAS :=
else
  PLUGIN      := $(BUILD)/libcptv_plugin.so
  VLC_LIBDIR  ?= $(firstword $(wildcard /usr/lib/$(shell $(CC) -dumpmachine)/vlc /usr/lib64/vlc /usr/lib/vlc /usr/local/lib/vlc))
  PLUGIN_DIR  ?= $(VLC_LIBDIR)/plugins/demux
  # Leave libvlccore symbols undefined (resolved from the host VLC process)
  # and keep the Rust staticlib's symbols private.
  LINK_FLAGS  := -shared -Wl,--exclude-libs,ALL -Wl,--gc-sections -s \
                 -lgcc_s -lutil -lrt -lpthread -lm -ldl -lc
  VLC_BIN     ?= vlc
  LIBVLCCORE  ?= $(firstword $(wildcard /usr/lib/$(shell $(CC) -dumpmachine)/libvlccore.so.* /usr/lib64/libvlccore.so.* /usr/lib/libvlccore.so.* /usr/local/lib/libvlccore.so.*))
  CACHE_GEN   := $(firstword $(wildcard $(VLC_LIBDIR)/vlc-cache-gen))
  # Installed alongside the plugin on Linux: the file-manager thumbnailer.
  LINUX_EXTRAS := $(THUMBNAILER)
endif

# Install targets that may run under sudo must not build anything: root
# usually has no cargo on PATH (or no rustup toolchain), and a build would
# leave root-owned files in target/. On Linux they only check that `make`
# has already been run; macOS installs don't use sudo, so they still build.
ifeq ($(UNAME_S),Darwin)
  INSTALL_PREREQS := $(PLUGIN)
else
  INSTALL_PREREQS :=
endif
need_built = @test -e "$(1)" || { echo "$(1) not found: run 'make' (without sudo) first" >&2; exit 1; }

# The ABI tag VLC looks for in plugins ("vlc_entry__<tag>"). Debian/Ubuntu use
# 3_0_0ft64; upstream builds (incl. macOS VLC.app) use 3_0_0f.
ifneq ($(LIBVLCCORE),)
  VLC_ABI ?= $(shell grep -aoE 'vlc_entry__3_0_0[a-z0-9]*' "$(LIBVLCCORE)" | head -n1 | sed 's/vlc_entry__//')
endif
ifeq ($(strip $(VLC_ABI)),)
  override VLC_ABI := 3_0_0f
endif
CFLAGS_ALL += -DCPTV_MODULE_SYMBOL=$(VLC_ABI)
ifneq ($(findstring t64,$(VLC_ABI)),)
  CFLAGS_ALL += -D_TIME_BITS=64
endif

.PHONY: all rust thumbnailer install uninstall install-user uninstall-user associate test clean distclean \
        quicklook install-quicklook uninstall-quicklook install-thumbnailer uninstall-thumbnailer

all: $(PLUGIN) $(LINUX_EXTRAS)

$(HEADERS_STAMP):
	./scripts/fetch-vlc-headers.sh $(VLC_VERSION) third_party/vlc-$(VLC_VERSION)

rust:
	$(CARGO) build --release --lib

$(RUST_LIB): rust

thumbnailer:
	$(CARGO) build --release --features thumbnailer --bin cptv-thumbnailer

$(THUMBNAILER): thumbnailer

$(BUILD)/cptv_demux.o: vlc/cptv_demux.c vlc/cptv_core.h Makefile $(HEADERS_STAMP) | $(BUILD)
	$(CC) $(CFLAGS_ALL) -c $< -o $@

$(PLUGIN): $(BUILD)/cptv_demux.o $(RUST_LIB)
	$(CC) -o $@ $(BUILD)/cptv_demux.o $(RUST_LIB) $(LINK_FLAGS) $(LDFLAGS)
ifeq ($(UNAME_S),Darwin)
	codesign --force --sign - $@
endif
	@echo "Built $@ (VLC plugin ABI $(VLC_ABI))"

$(BUILD):
	mkdir -p $@

install: $(INSTALL_PREREQS)
	$(call need_built,$(PLUGIN))
	@test -n "$(PLUGIN_DIR)" || { echo "Could not find VLC's plugin dir; pass PLUGIN_DIR=..."; exit 1; }
ifeq ($(UNAME_S),Darwin)
	@# VLC.app's plugins dir already exists; `install -d` would chmod it, which
	@# macOS App Management refuses inside an app bundle. Plain cp only writes the file.
	@cp $(PLUGIN) "$(PLUGIN_DIR)/" || { \
	    echo "Copy into $(PLUGIN_DIR) was refused. Give your terminal app permission under"; \
	    echo "System Settings > Privacy & Security > App Management, then re-run."; exit 1; }
else
	install -d "$(PLUGIN_DIR)"
	install -m 0644 $(PLUGIN) "$(PLUGIN_DIR)/"
	@$(MAKE) --no-print-directory install-thumbnailer
endif
ifneq ($(CACHE_GEN),)
	-$(CACHE_GEN) "$(dir $(PLUGIN_DIR:/=))"
endif
	@echo "Installed $(notdir $(PLUGIN)) to $(PLUGIN_DIR)"

uninstall:
	rm -f "$(PLUGIN_DIR)/$(notdir $(PLUGIN))"
ifneq ($(UNAME_S),Darwin)
	@$(MAKE) --no-print-directory uninstall-thumbnailer
endif
ifneq ($(CACHE_GEN),)
	-$(CACHE_GEN) "$(dir $(PLUGIN_DIR:/=))"
endif

# --- Linux file-manager thumbnailer, system-wide ------------------------------
# GNOME (and Cinnamon) run thumbnailers in a bubblewrap sandbox that only
# mounts /usr, so the binary has to live there; $(PREFIX)/share is on the
# desktop's XDG_DATA_DIRS. No build here (see INSTALL_PREREQS).
install-thumbnailer:
	$(call need_built,$(THUMBNAILER))
	install -d "$(PREFIX)/bin" "$(PREFIX)/share/thumbnailers"
	install -m 0755 $(THUMBNAILER) "$(PREFIX)/bin/"
	sed 's|@BIN@|$(PREFIX)/bin/cptv-thumbnailer|g' packaging/linux/cptv.thumbnailer.in \
	    > "$(PREFIX)/share/thumbnailers/cptv.thumbnailer"
	@echo "Installed $(PREFIX)/bin/cptv-thumbnailer. Restart the file manager (nautilus -q)."

uninstall-thumbnailer:
	rm -f "$(PREFIX)/bin/cptv-thumbnailer" "$(PREFIX)/share/thumbnailers/cptv.thumbnailer"

# --- Per-user install: no root needed, VLC itself untouched -------------------
XDG_DATA_HOME   ?= $(HOME)/.local/share
XDG_CONFIG_HOME ?= $(HOME)/.config
USER_LIB_DIR    ?= $(HOME)/.local/lib/cptv-vlc
USER_PLUGIN_DIR ?= $(USER_LIB_DIR)/plugins
VLC_PATH        ?= $(shell command -v vlc 2>/dev/null || echo vlc)
# macOS: VLC 3 has no per-user plugin dir, so a LaunchAgent sets VLC_PLUGIN_PATH
# in the login session (launchctl setenv); every VLC started from Finder/Dock
# then loads the plugin, and .cptv files open in the one normal VLC.
MAC_PLUGIN_DIR  ?= $(HOME)/Library/Application Support/cptv-vlc/plugins
AGENT_LABEL     := nz.org.cacophony.cptv-vlc
AGENT_PLIST     := $(HOME)/Library/LaunchAgents/$(AGENT_LABEL).plist

install-user: $(PLUGIN) $(LINUX_EXTRAS)
ifeq ($(UNAME_S),Darwin)
	mkdir -p "$(MAC_PLUGIN_DIR)" "$(HOME)/Library/LaunchAgents"
	cp $(PLUGIN) "$(MAC_PLUGIN_DIR)/"
	sed 's|@PLUGIN_DIR@|$(MAC_PLUGIN_DIR)|g' packaging/macos/$(AGENT_LABEL).plist.in > "$(AGENT_PLIST)"
	@launchctl bootout gui/$$(id -u) "$(AGENT_PLIST)" 2>/dev/null || true
	launchctl bootstrap gui/$$(id -u) "$(AGENT_PLIST)"
	launchctl setenv VLC_PLUGIN_PATH "$(MAC_PLUGIN_DIR)"
	swift scripts/macos-set-default.swift cptv org.videolan.vlc
	@echo "Done: double-clicking a .cptv file now opens it in VLC with the CPTV plugin."
	@if pgrep -qx VLC; then echo "Quit and reopen VLC once so it picks up the plugin."; fi
else
	install -d "$(USER_PLUGIN_DIR)" "$(XDG_DATA_HOME)/applications" "$(XDG_DATA_HOME)/mime/packages" "$(XDG_CONFIG_HOME)"
	install -m 0644 $(PLUGIN) "$(USER_PLUGIN_DIR)/"
	install -m 0644 packaging/linux/cptv-mime.xml "$(XDG_DATA_HOME)/mime/packages/cptv.xml"
	sed -e 's|@PLUGIN_DIR@|$(USER_PLUGIN_DIR)|g' -e 's|@VLC@|$(VLC_PATH)|g' \
	    packaging/linux/vlc-cptv.desktop.in > "$(XDG_DATA_HOME)/applications/vlc-cptv.desktop"
	-update-mime-database "$(XDG_DATA_HOME)/mime"
	-update-desktop-database "$(XDG_DATA_HOME)/applications"
	xdg-mime default vlc-cptv.desktop video/x-cptv
	@# Thumbnails: where file managers sandbox thumbnailers with bubblewrap
	@# (GNOME, Cinnamon) a copy in $$HOME can't run, and a per-user .thumbnailer
	@# entry would override a working one in /usr/local, so don't install one.
	@if command -v bwrap >/dev/null 2>&1; then \
	    rm -f "$(USER_LIB_DIR)/cptv-thumbnailer" "$(XDG_DATA_HOME)/thumbnailers/cptv.thumbnailer"; \
	    echo "Done: double-clicking a .cptv file now opens it in VLC with the CPTV plugin."; \
	    echo "For file-manager thumbnails, also run: sudo make install-thumbnailer"; \
	    echo "(this desktop sandboxes thumbnailers, so they must be installed under /usr)."; \
	else \
	    install -d "$(USER_LIB_DIR)" "$(XDG_DATA_HOME)/thumbnailers" && \
	    install -m 0755 $(THUMBNAILER) "$(USER_LIB_DIR)/" && \
	    sed 's|@BIN@|$(USER_LIB_DIR)/cptv-thumbnailer|g' packaging/linux/cptv.thumbnailer.in \
	        > "$(XDG_DATA_HOME)/thumbnailers/cptv.thumbnailer" && \
	    echo "Done: double-clicking a .cptv file now opens it in VLC with the CPTV plugin," && \
	    echo "and file managers can show .cptv thumbnails."; \
	fi
endif

uninstall-user:
ifeq ($(UNAME_S),Darwin)
	-launchctl bootout gui/$$(id -u) "$(AGENT_PLIST)" 2>/dev/null
	-launchctl unsetenv VLC_PLUGIN_PATH
	rm -f "$(AGENT_PLIST)"
	rm -rf "$(HOME)/Library/Application Support/cptv-vlc"
else
	rm -f "$(USER_PLUGIN_DIR)/$(notdir $(PLUGIN))" \
	      "$(USER_LIB_DIR)/cptv-thumbnailer" \
	      "$(XDG_DATA_HOME)/thumbnailers/cptv.thumbnailer" \
	      "$(XDG_DATA_HOME)/applications/vlc-cptv.desktop" \
	      "$(XDG_DATA_HOME)/mime/packages/cptv.xml"
	-update-mime-database "$(XDG_DATA_HOME)/mime"
	-update-desktop-database "$(XDG_DATA_HOME)/applications"
endif

# --- macOS Quick Look extensions ---------------------------------------------
QL_APP_NAME     := CPTV Quick Look.app
QL_BUILD        := $(BUILD)/$(QL_APP_NAME)
QL_APP          ?= $(HOME)/Applications/$(QL_APP_NAME)
LSREGISTER      := /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

quicklook: rust | $(BUILD)
	./macos/quicklook/build.sh $(RUST_LIB) "$(QL_BUILD)"
	@# Only the installed copy should be registered with LaunchServices.
	@$(LSREGISTER) -u "$(CURDIR)/$(QL_BUILD)" 2>/dev/null || true

install-quicklook: quicklook
	mkdir -p "$(HOME)/Applications"
	rm -rf "$(QL_APP)"
	ditto "$(QL_BUILD)" "$(QL_APP)"
	$(LSREGISTER) -f -R "$(QL_APP)"
	@# pkd handles the removal of the old copy asynchronously and can drop the
	@# new registration after we add it, so add, wait, check, and retry.
	@for i in 1 2 3 4 5 6 7 8 9 10; do \
	    pluginkit -a "$(QL_APP)/Contents/PlugIns/CPTVThumbnail.appex"; \
	    pluginkit -a "$(QL_APP)/Contents/PlugIns/CPTVPreview.appex"; \
	    sleep 1; \
	    if pluginkit -m -i nz.org.cacophony.cptv-quicklook.thumbnail | grep -q thumbnail && \
	       pluginkit -m -i nz.org.cacophony.cptv-quicklook.preview | grep -q preview; then \
	        echo "Quick Look extensions registered"; break; \
	    fi; \
	    if [ $$i = 10 ]; then echo "Quick Look extensions failed to register" >&2; exit 1; fi; \
	done
	@# The app names the .cptv type, so point that type back at VLC for opening.
	swift scripts/macos-set-default.swift cptv org.videolan.vlc
	@qlmanage -r >/dev/null; qlmanage -r cache >/dev/null
	@echo "Done: Finder shows .cptv thumbnails, and Space previews them."

uninstall-quicklook:
	@pluginkit -r "$(QL_APP)/Contents/PlugIns/CPTVThumbnail.appex" 2>/dev/null || true
	@pluginkit -r "$(QL_APP)/Contents/PlugIns/CPTVPreview.appex" 2>/dev/null || true
	@$(LSREGISTER) -u "$(QL_APP)" 2>/dev/null || true
	rm -rf "$(QL_APP)"
	@qlmanage -r >/dev/null; qlmanage -r cache >/dev/null

# --- File association for a system-wide (or macOS) install --------------------
associate:
ifeq ($(UNAME_S),Darwin)
	@if command -v duti >/dev/null; then \
	    duti -s org.videolan.vlc .cptv all && echo ".cptv files now open with VLC"; \
	else \
	    echo "Install duti (brew install duti) and re-run, or: select a .cptv file in Finder,"; \
	    echo "Get Info (Cmd-I) > Open with > VLC > Change All..."; \
	fi
else
	install -d "$(XDG_DATA_HOME)/mime/packages" "$(XDG_CONFIG_HOME)"
	install -m 0644 packaging/linux/cptv-mime.xml "$(XDG_DATA_HOME)/mime/packages/cptv.xml"
	-update-mime-database "$(XDG_DATA_HOME)/mime"
	xdg-mime default vlc.desktop video/x-cptv
	@echo ".cptv files now open with VLC"
endif

test: $(PLUGIN) $(THUMBNAILER)
	$(CARGO) test --release
	./scripts/smoke-test.sh $(PLUGIN) "$$(./scripts/fixtures-dir.sh)/20240507-074536.cptv" "$(VLC_BIN)"
	$(THUMBNAILER) -s 256 "$$(./scripts/fixtures-dir.sh)/20240507-074536.cptv" $(BUILD)/test-thumbnail.png
	@[ "$$(head -c 8 $(BUILD)/test-thumbnail.png | od -An -tx1 | tr -d ' \n')" = 89504e470d0a1a0a ] \
	    && echo "OK: thumbnailer wrote a PNG" || { echo "FAIL: thumbnailer output isn't a PNG" >&2; exit 1; }

clean:
	rm -rf $(BUILD)
	$(CARGO) clean

distclean: clean
	rm -rf third_party
