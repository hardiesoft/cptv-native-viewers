# Shared by the installer's postinstall scripts and uninstall.sh (sourced, so
# no shebang). They run as root; per-user steps run for whoever is logged in
# at the console. If nobody is (e.g. an MDM install at the login window), the
# LaunchAgent still takes effect at the next login.

LABEL=nz.org.cacophony.cptv-native-viewers
AGENT="/Library/LaunchAgents/$LABEL.plist"
SUPPORT="/Library/Application Support/cptv-native-viewers"
PLUGIN_DIR="$SUPPORT/plugins"
QL_APP="/Applications/CPTV Quick Look.app"
QL_EXTS="CPTVThumbnail CPTVPreview"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

CONSOLE_USER="$(stat -f %Su /dev/console 2>/dev/null || echo root)"
CONSOLE_UID="$(id -u "$CONSOLE_USER" 2>/dev/null || echo 0)"
if [ "$CONSOLE_USER" = root ] || [ "$CONSOLE_USER" = loginwindow ]; then
    CONSOLE_UID=0
fi
CONSOLE_HOME=""
if [ "$CONSOLE_UID" != 0 ]; then
    CONSOLE_HOME="$(dscl . -read "/Users/$CONSOLE_USER" NFSHomeDirectory 2>/dev/null | awk '{print $2}')"
fi

have_user() { [ "$CONSOLE_UID" != 0 ]; }

# Run a command as the console user, in their login session.
as_user() { launchctl asuser "$CONSOLE_UID" sudo -u "$CONSOLE_USER" "$@"; }

log() { echo "cptv-native-viewers: $*"; }
