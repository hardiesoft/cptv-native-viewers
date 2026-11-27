#!/bin/sh
# Cross-build the Windows x64 pieces from Linux with MinGW-w64:
#   build/windows/libcptv_plugin.dll        VLC 3 plugin
#   build/windows/cptv_thumbnail_handler.dll  Explorer thumbnail handler (COM)
#   build/windows/cptv-thumbnail-test.exe   test tool for the handler (not shipped)
# and, if Inno Setup's compiler is available (ISCC, e.g. under Wine or on a
# Windows runner), the installer dist/cptv-native-viewers-<version>-windows-x64.exe.
#
#   scripts/package-windows.sh [plugin|thumbnailer|build|installer|all]
#   (default: build = plugin + thumbnailer)
#
# Needs: rustup (adds the x86_64-pc-windows-gnu target), x86_64-w64-mingw32-gcc
# (Debian/Ubuntu: gcc-mingw-w64-x86-64), curl, 7z (p7zip-full).
#
# Environment (all optional):
#   VERSION     installer version (default: Cargo.toml's)
#   MINGW_CC    C compiler (default: x86_64-w64-mingw32-gcc)
#   ISCC        Inno Setup compiler command (default: iscc). Under Wine, e.g.
#               ISCC="wine /path/to/ISCC.exe" (the command is split on spaces,
#               so link the "Inno Setup 6" folder somewhere without them).
set -eu
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
WHAT="${1:-build}"
VERSION="${VERSION:-$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n1)}"
MINGW_CC="${MINGW_CC:-x86_64-w64-mingw32-gcc}"
TARGET=x86_64-pc-windows-gnu
OUT=build/windows

# The official VLC Windows build ships the plugin SDK (headers + MinGW import
# library for libvlccore.dll). Pinned by checksum.
VLC_VERSION=3.0.23
VLC_7Z_SHA256=eb4fd8a28291da73608c733786a09610fea865fbe94113bcb60b91c1ebb8404a
VLC_SDK="third_party/vlc-$VLC_VERSION-win64-sdk"

fetch_sdk() {
    [ -f "$VLC_SDK/lib/libvlccore.lib" ] && return 0
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    url="https://download.videolan.org/pub/videolan/vlc/$VLC_VERSION/win64/vlc-$VLC_VERSION-win64.7z"
    echo "Fetching VLC $VLC_VERSION Windows SDK from $url"
    curl -sSfL -o "$tmp/vlc.7z" "$url"
    echo "$VLC_7Z_SHA256  $tmp/vlc.7z" | sha256sum -c --quiet
    7z x -y -o"$tmp" "$tmp/vlc.7z" "vlc-$VLC_VERSION/sdk/*" >/dev/null
    rm -rf "$VLC_SDK"
    mkdir -p "$(dirname "$VLC_SDK")"
    mv "$tmp/vlc-$VLC_VERSION/sdk" "$VLC_SDK"
    rm -rf "$tmp"
    trap - EXIT
}

prepare() {
    command -v "$MINGW_CC" >/dev/null || {
        echo "$MINGW_CC not found (Debian/Ubuntu: apt install gcc-mingw-w64-x86-64)" >&2; exit 1; }
    rustup target add "$TARGET" >/dev/null 2>&1 || true
    mkdir -p "$OUT"
}

# Only system DLLs (and VLC's core, for the plugin) may be imported.
check_imports() {
    objdump="${MINGW_CC%gcc*}objdump"
    command -v "$objdump" >/dev/null || objdump=x86_64-w64-mingw32-objdump
    bad=$("$objdump" -p "$1" | sed -n 's/^\tDLL Name: //p' | tr 'A-Z' 'a-z' \
        | grep -vE '^(kernel32|ntdll|userenv|ws2_32|dbghelp|msvcrt|advapi32|ole32|oleaut32|shell32|shlwapi|user32|gdi32|bcryptprimitives|propsys|combase|api-ms-win-.*|libvlccore)\.dll$' || true)
    [ -z "$bad" ] || { echo "$1 imports unexpected DLLs: $bad" >&2; exit 1; }
}

plugin() {
    prepare
    fetch_sdk
    cargo build --release --lib --target "$TARGET"
    # Windows plugins must link against libvlccore's import library (no
    # undefined symbols in a DLL), use VLC's C runtime (msvcrt, MinGW's
    # default: plugin and core free each other's strings) and the upstream
    # ABI tag. -static keeps libgcc/winpthread inside the DLL.
    "$MINGW_CC" -std=gnu11 -O2 -Wall -Wextra -Wno-unused-parameter -Wno-sign-compare \
        -D__PLUGIN__ -D_FILE_OFFSET_BITS=64 -D_REENTRANT -D_THREAD_SAFE \
        -DCPTV_MODULE_SYMBOL=3_0_0f \
        -I"$VLC_SDK/include/vlc/plugins" -Ivlc \
        -c vlc/cptv_demux.c -o "$OUT/cptv_demux.o"
    "$MINGW_CC" -shared -static -s -o "$OUT/libcptv_plugin.dll" "$OUT/cptv_demux.o" \
        "target/$TARGET/release/libcptv_native_viewers.a" \
        -L"$VLC_SDK/lib" -lvlccore \
        -lkernel32 -lntdll -luserenv -lws2_32 -ldbghelp -lsynchronization \
        -Wl,--exclude-libs,ALL
    check_imports "$OUT/libcptv_plugin.dll"
    echo "Built $OUT/libcptv_plugin.dll"
}

thumbnailer() {
    prepare
    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="$MINGW_CC" \
        cargo build --release --target "$TARGET" \
        --manifest-path windows/thumbnail-handler/Cargo.toml
    cp "windows/thumbnail-handler/target/$TARGET/release/cptv_thumbnail_handler.dll" \
       "windows/thumbnail-handler/target/$TARGET/release/cptv-thumbnail-test.exe" "$OUT/"
    check_imports "$OUT/cptv_thumbnail_handler.dll"
    echo "Built $OUT/cptv_thumbnail_handler.dll"
}

installer() {
    iscc="${ISCC:-iscc}"
    # shellcheck disable=SC2086
    set -- $iscc
    command -v "$1" >/dev/null 2>&1 || {
        echo "Inno Setup compiler ($iscc) not found; set ISCC" >&2; exit 1; }
    mkdir -p dist
    # Under Wine, ISCC needs Windows paths (Z:\...).
    winpath() { if [ "$(basename "$1")" = wine ]; then winepath -w "$2"; else echo "$2"; fi; }
    "$@" /Q "/DAppVersion=$VERSION" \
        "/DBuildDir=$(winpath "$1" "$ROOT/$OUT")" "/DOutDir=$(winpath "$1" "$ROOT/dist")" \
        "$(winpath "$1" "$ROOT/packaging/windows/cptv-native-viewers.iss")"
    echo "Built dist/cptv-native-viewers-$VERSION-windows-x64.exe"
}

case "$WHAT" in
    plugin) plugin ;;
    thumbnailer) thumbnailer ;;
    build) plugin; thumbnailer ;;
    installer) installer ;;
    all) plugin; thumbnailer; installer ;;
    *) echo "usage: $0 [plugin|thumbnailer|build|installer|all]" >&2; exit 2 ;;
esac
