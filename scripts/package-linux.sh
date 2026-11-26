#!/bin/sh
# Build a Linux package with nfpm (https://nfpm.goreleaser.com) on the distro
# it targets, after `make`:
#   package-linux.sh deb|rpm <plugin.so> <cptv-thumbnailer> <plugin dir>
# Writes dist/cptv-native-viewers_<version>_<arch>.deb or
# dist/cptv-native-viewers-<version>-1.<arch>.rpm (nfpm's naming).
#
# Environment: VERSION (default: Cargo.toml's), MAINTAINER.
set -eu
FORMAT="$1"; PLUGIN="$2"; THUMBNAILER="$3"; PLUGIN_DIR="$4"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="${VERSION:-$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n1)}"
MAINTAINER="${MAINTAINER:-The Cacophony Project}"
case "$FORMAT" in
    deb) DEPENDS="libvlccore9" ;;   # Debian/Ubuntu
    rpm) DEPENDS="vlc-libs" ;;      # Fedora
    *) echo "usage: $0 deb|rpm <plugin.so> <cptv-thumbnailer> <plugin dir>" >&2; exit 2 ;;
esac
case "$(uname -m)" in
    x86_64) ARCH=amd64 ;;
    aarch64|arm64) ARCH=arm64 ;;
    *) ARCH="$(uname -m)" ;;
esac
[ -n "$PLUGIN_DIR" ] || { echo "No VLC plugin dir found; is VLC installed?" >&2; exit 1; }
command -v nfpm >/dev/null || { echo "nfpm not found (https://nfpm.goreleaser.com)" >&2; exit 1; }

OUT="build/linux-pkg"
mkdir -p "$OUT" dist
sed 's|@BIN@|/usr/bin/cptv-thumbnailer|g' packaging/linux/cptv.thumbnailer.in > "$OUT/cptv.thumbnailer"

cat > "$OUT/nfpm.yaml" <<EOF
name: cptv-native-viewers
arch: $ARCH
platform: linux
version: $VERSION
section: video
priority: optional
maintainer: $MAINTAINER
description: |
  Viewers for Cacophony Project CPTV thermal video files.
  A VLC plugin that plays .cptv files, and a thumbnailer so file managers
  (GNOME Files, Nemo, Caja, Thunar) show previews of them.
vendor: The Cacophony Project
homepage: https://github.com/hardiesoft/cptv-native-viewers
license: LGPL-2.1-or-later
depends:
  - $DEPENDS
contents:
  - src: $PLUGIN
    dst: $PLUGIN_DIR/libcptv_plugin.so
    file_info: { mode: 0644 }
  - src: $THUMBNAILER
    dst: /usr/bin/cptv-thumbnailer
    file_info: { mode: 0755 }
  - src: $OUT/cptv.thumbnailer
    dst: /usr/share/thumbnailers/cptv.thumbnailer
    file_info: { mode: 0644 }
  - src: packaging/linux/cptv-mime.xml
    dst: /usr/share/mime/packages/cptv.xml
    file_info: { mode: 0644 }
  - src: packaging/linux/vlc-cptv-system.desktop
    dst: /usr/share/applications/vlc-cptv.desktop
    file_info: { mode: 0644 }
scripts:
  postinstall: packaging/linux/postinstall.sh
  postremove: packaging/linux/postinstall.sh
EOF

nfpm package --config "$OUT/nfpm.yaml" --packager "$FORMAT" --target dist/
