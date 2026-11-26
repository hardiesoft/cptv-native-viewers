#!/bin/sh
# Fetch the public VLC plugin headers (include/) for a given VLC 3.0.x release.
# Neither VLC.app on macOS nor most Linux installs ship them, and they are all
# that is needed to build an out-of-tree plugin.
set -eu
VERSION="${1:-3.0.23}"
DEST="${2:-third_party/vlc-$VERSION}"
if [ -f "$DEST/include/vlc_plugin.h" ]; then
    exit 0
fi
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
git clone --quiet --depth 1 --branch "$VERSION" --filter=blob:none --sparse \
    https://code.videolan.org/videolan/vlc.git "$TMP/vlc"
git -C "$TMP/vlc" sparse-checkout set include
mkdir -p "$DEST"
rm -rf "$DEST/include"
cp -R "$TMP/vlc/include" "$DEST/include"
cp "$TMP/vlc/COPYING.LIB" "$DEST/" 2>/dev/null || true
echo "VLC $VERSION headers in $DEST/include"
