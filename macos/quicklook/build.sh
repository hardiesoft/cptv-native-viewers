#!/bin/sh
# Build "CPTV Quick Look.app": a host app declaring the .cptv file type, with a
# Quick Look thumbnail extension and a preview extension inside. Built with
# plain swiftc (no Xcode project) and signed ad hoc to run on this Mac.
#   usage: build.sh <libcptv_vlc.a> <output .app>
set -eu
RUST_LIB="$1"
APP="$2"
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
BUNDLE_ID=nz.org.cacophony.cptv-quicklook
UTI=nz.org.cacophony.cptv
MIN_OS=13.0
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n1)"
ARCH="$(uname -m)"

SWIFTC="xcrun swiftc -target $ARCH-apple-macos$MIN_OS -swift-version 5 -O \
    -import-objc-header $ROOT/vlc/cptv_core.h"
# Rust's std needs these when linking the staticlib.
RUST_LINK="$RUST_LIB -framework CoreFoundation -liconv"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/PlugIns" "$APP/Contents/Resources"

plist_common() {  # <name> <bundle id> <executable> <package type>
    cat <<EOF
	<key>CFBundleDevelopmentRegion</key><string>en</string>
	<key>CFBundleName</key><string>$1</string>
	<key>CFBundleDisplayName</key><string>$1</string>
	<key>CFBundleIdentifier</key><string>$2</string>
	<key>CFBundleExecutable</key><string>$3</string>
	<key>CFBundlePackageType</key><string>$4</string>
	<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
	<key>CFBundleShortVersionString</key><string>$VERSION</string>
	<key>CFBundleVersion</key><string>$VERSION</string>
	<key>LSMinimumSystemVersion</key><string>$MIN_OS</string>
EOF
}

# --- Extensions ---------------------------------------------------------------
build_ext() {  # <name> <bundle suffix> <extension point> <principal class> <frameworks> <extra attrs> <sources...>
    name="$1"; suffix="$2"; point="$3"; class="$4"; frameworks="$5"; attrs="$6"
    shift 6
    ext="$APP/Contents/PlugIns/$name.appex"
    mkdir -p "$ext/Contents/MacOS"
    # shellcheck disable=SC2086
    $SWIFTC -module-name "$name" -application-extension -parse-as-library \
        -Xlinker -e -Xlinker _NSExtensionMain \
        "$HERE/Shared/CPTVClip.swift" "$@" $RUST_LINK $frameworks \
        -o "$ext/Contents/MacOS/$name"
    {
        echo '<?xml version="1.0" encoding="UTF-8"?>'
        echo '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">'
        echo '<plist version="1.0"><dict>'
        plist_common "$name" "$BUNDLE_ID.$suffix" "$name" XPC!
        cat <<EOF
	<key>NSExtension</key>
	<dict>
		<key>NSExtensionPointIdentifier</key><string>$point</string>
		<key>NSExtensionPrincipalClass</key><string>$class</string>
		<key>NSExtensionAttributes</key>
		<dict>
			<key>QLSupportedContentTypes</key><array><string>$UTI</string></array>
			$attrs
		</dict>
	</dict>
</dict></plist>
EOF
    } > "$ext/Contents/Info.plist"
    codesign --force --sign - --entitlements "$HERE/extension.entitlements" "$ext"
}

build_ext CPTVThumbnail thumbnail com.apple.quicklook.thumbnail ThumbnailProvider \
    "-framework QuickLookThumbnailing" \
    "<key>QLThumbnailMinimumDimension</key><integer>0</integer>" \
    "$HERE/Thumbnail/ThumbnailProvider.swift"

build_ext CPTVPreview preview com.apple.quicklook.preview PreviewViewController \
    "-framework QuickLookUI -framework AppKit" \
    "<key>QLSupportsSearchableItems</key><false/>" \
    "$HERE/Preview/PreviewViewController.swift"

# --- Host app -----------------------------------------------------------------
xcrun swiftc -target "$ARCH-apple-macos$MIN_OS" -swift-version 5 -O \
    "$HERE/App/main.swift" -o "$APP/Contents/MacOS/CPTV Quick Look"
for icns in /Applications/VLC.app/Contents/Resources/VLC.icns; do
    [ -f "$icns" ] && cp "$icns" "$APP/Contents/Resources/AppIcon.icns"
done
{
    echo '<?xml version="1.0" encoding="UTF-8"?>'
    echo '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">'
    echo '<plist version="1.0"><dict>'
    plist_common "CPTV Quick Look" "$BUNDLE_ID" "CPTV Quick Look" APPL
    cat <<EOF
	<key>CFBundleIconFile</key><string>AppIcon</string>
	<key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
	<!-- Names the .cptv type so Quick Look extensions can claim it. Opening
	     files stays with VLC (this app declares no document types). -->
	<key>UTExportedTypeDeclarations</key>
	<array><dict>
		<key>UTTypeIdentifier</key><string>$UTI</string>
		<key>UTTypeDescription</key><string>CPTV thermal video</string>
		<key>UTTypeConformsTo</key><array><string>public.data</string><string>public.content</string></array>
		<key>UTTypeTagSpecification</key><dict>
			<key>public.filename-extension</key><array><string>cptv</string></array>
			<key>public.mime-type</key><array><string>video/x-cptv</string></array>
		</dict>
	</dict></array>
</dict></plist>
EOF
} > "$APP/Contents/Info.plist"
codesign --force --sign - "$APP"
echo "Built $APP"
