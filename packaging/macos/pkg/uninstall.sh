#!/bin/sh
# Removes everything the "CPTV Native Viewers" installer put on this Mac:
#   sudo "/Library/Application Support/cptv-native-viewers/uninstall.sh"
set -u
if [ "$(id -u)" != 0 ]; then
    echo "Run this with sudo:  sudo \"$0\"" >&2
    exit 1
fi
. "$(dirname "$0")/common.sh"

if have_user; then
    launchctl bootout "gui/$CONSOLE_UID/$LABEL" 2>/dev/null
    launchctl asuser "$CONSOLE_UID" launchctl unsetenv VLC_PLUGIN_PATH
    if [ -d "$QL_APP" ]; then
        for e in $QL_EXTS; do as_user pluginkit -r "$QL_APP/Contents/PlugIns/$e.appex" 2>/dev/null; done
        as_user "$LSREGISTER" -u "$QL_APP" 2>/dev/null
    fi
fi
rm -f "$AGENT"
rm -rf "$QL_APP"
pkgutil --forget nz.org.cacophony.cptv-native-viewers.plugin >/dev/null 2>&1
pkgutil --forget nz.org.cacophony.cptv-native-viewers.quicklook >/dev/null 2>&1
if have_user; then
    as_user qlmanage -r cache >/dev/null 2>&1
fi
# Last, since this script lives here.
rm -rf "$SUPPORT"
echo "CPTV Native Viewers has been removed. Quit and reopen VLC if it's running."
