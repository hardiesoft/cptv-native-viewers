#!/bin/sh
# Build the macOS installer: dist/cptv-native-viewers-<version>-macos.pkg, a universal
# (Apple silicon + Intel) package with two components:
#   plugin     VLC plugin + LaunchAgent (/Library), .cptv → VLC, uninstall.sh
#   quicklook  "CPTV Quick Look.app" in /Applications (macOS 13+)
#
# Environment (all optional):
#   VERSION              package version (default: Cargo.toml's)
#   SIGN_APP             "Developer ID Application: …" identity for code; default ad hoc
#   SIGN_PKG             "Developer ID Installer: …" identity for the .pkg; default unsigned
#   NOTARY_KEYCHAIN_PROFILE, or APPLE_ID + APPLE_TEAM_ID + APPLE_APP_PASSWORD:
#                        notarize and staple the .pkg (needs SIGN_APP and SIGN_PKG)
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="${VERSION:-$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n1)}"
SIGN_APP="${SIGN_APP:-}"
SIGN_PKG="${SIGN_PKG:-}"
OUT="build/pkg"
DIST="dist"
PKG="$DIST/cptv-native-viewers-$VERSION-macos.pkg"
PKGDIR="packaging/macos/pkg"
SUPPORT_REL="Library/Application Support/cptv-native-viewers"

rm -rf "$OUT"
mkdir -p "$OUT" "$DIST"

# --- Build each architecture, then merge --------------------------------------
for pair in arm64:aarch64-apple-darwin x86_64:x86_64-apple-darwin; do
    arch="${pair%%:*}"; target="${pair#*:}"
    rustup target add "$target" >/dev/null 2>&1 || true
    make --no-print-directory BUILD="build/$arch" ARCH="$arch" RUST_TARGET="$target" \
        "build/$arch/libcptv_plugin.dylib" rust
done
mkdir -p build/universal
lipo -create build/arm64/libcptv_plugin.dylib build/x86_64/libcptv_plugin.dylib \
    -output build/universal/libcptv_plugin.dylib
lipo -create target/aarch64-apple-darwin/release/libcptv_native_viewers.a \
    target/x86_64-apple-darwin/release/libcptv_native_viewers.a -output build/universal/libcptv_native_viewers.a

# .cptv → VLC helper (the Makefile runs the .swift with `swift`; users have no toolchain).
for arch in arm64 x86_64; do
    xcrun swiftc -O -target "$arch-apple-macos11.0" scripts/macos-set-default.swift \
        -o "build/universal/cptv-set-default.$arch"
done
lipo -create build/universal/cptv-set-default.arm64 build/universal/cptv-set-default.x86_64 \
    -output build/universal/cptv-set-default

# Quick Look app. No VLC icon in a distributed build (VideoLAN's trademark).
ARCHS="arm64 x86_64" VERSION="$VERSION" APP_ICON=/nonexistent \
    ./macos/quicklook/build.sh build/universal/libcptv_native_viewers.a "build/universal/CPTV Quick Look.app"

# --- Payloads -------------------------------------------------------------------
P="$OUT/plugin-root"
mkdir -p "$P/$SUPPORT_REL/plugins" "$P/Library/LaunchAgents"
cp build/universal/libcptv_plugin.dylib "$P/$SUPPORT_REL/plugins/"
cp build/universal/cptv-set-default "$P/$SUPPORT_REL/"
cp "$PKGDIR/uninstall.sh" "$PKGDIR/common.sh" "$P/$SUPPORT_REL/"
chmod 755 "$P/$SUPPORT_REL/uninstall.sh" "$P/$SUPPORT_REL/cptv-set-default"
sed "s|@PLUGIN_DIR@|/$SUPPORT_REL/plugins|g" packaging/macos/nz.org.cacophony.cptv-native-viewers.plist.in \
    > "$P/Library/LaunchAgents/nz.org.cacophony.cptv-native-viewers.plist"

Q="$OUT/quicklook-root"
mkdir -p "$Q/Applications"
ditto "build/universal/CPTV Quick Look.app" "$Q/Applications/CPTV Quick Look.app"

for c in plugin quicklook; do
    mkdir -p "$OUT/$c-scripts"
    cp "$PKGDIR/$c-postinstall" "$OUT/$c-scripts/postinstall"
    cp "$PKGDIR/common.sh" "$OUT/$c-scripts/"
    chmod 755 "$OUT/$c-scripts/postinstall"
done

# --- Code signing -------------------------------------------------------------------
if [ -n "$SIGN_APP" ]; then
    echo "Signing code with: $SIGN_APP"
    sign() { codesign --force --timestamp --options runtime --sign "$SIGN_APP" "$@"; }
    sign "$P/$SUPPORT_REL/plugins/libcptv_plugin.dylib"
    sign "$P/$SUPPORT_REL/cptv-set-default"
    APP="$Q/Applications/CPTV Quick Look.app"
    for e in CPTVThumbnail CPTVPreview; do
        sign --entitlements macos/quicklook/extension.entitlements "$APP/Contents/PlugIns/$e.appex"
    done
    sign "$APP"
fi   # otherwise everything keeps its ad-hoc signature from the build

# --- Component packages -------------------------------------------------------------
# Not relocatable: otherwise Installer "updates" a same-ID app it finds elsewhere
# (e.g. a per-user ~/Applications copy) instead of installing to /Applications.
pkgbuild --analyze --root "$Q" "$OUT/quicklook-components.plist" >/dev/null
plutil -replace 0.BundleIsRelocatable -bool false "$OUT/quicklook-components.plist"

pkgbuild --root "$P" --scripts "$OUT/plugin-scripts" --identifier nz.org.cacophony.cptv-native-viewers.plugin \
    --version "$VERSION" --install-location / "$OUT/plugin.pkg"
pkgbuild --root "$Q" --scripts "$OUT/quicklook-scripts" --identifier nz.org.cacophony.cptv-native-viewers.quicklook \
    --version "$VERSION" --install-location / --component-plist "$OUT/quicklook-components.plist" \
    "$OUT/quicklook.pkg"

sed "s|@VERSION@|$VERSION|g" "$PKGDIR/distribution.xml.in" > "$OUT/distribution.xml"
rm -f "$PKG"
if [ -n "$SIGN_PKG" ]; then
    echo "Signing package with: $SIGN_PKG"
    productbuild --distribution "$OUT/distribution.xml" --package-path "$OUT" \
        --resources "$PKGDIR/resources" --sign "$SIGN_PKG" --timestamp "$PKG"
else
    productbuild --distribution "$OUT/distribution.xml" --package-path "$OUT" \
        --resources "$PKGDIR/resources" "$PKG"
fi

# --- Notarization -------------------------------------------------------------------
if [ -n "${NOTARY_KEYCHAIN_PROFILE:-}" ]; then
    xcrun notarytool submit "$PKG" --keychain-profile "$NOTARY_KEYCHAIN_PROFILE" --wait
    xcrun stapler staple "$PKG"
elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
    xcrun notarytool submit "$PKG" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" \
        --password "$APPLE_APP_PASSWORD" --wait
    xcrun stapler staple "$PKG"
else
    echo "Not notarized: macOS will ask users to allow it under Privacy & Security."
fi

echo "Built $PKG"
