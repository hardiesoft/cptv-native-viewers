#!/bin/sh
# Play a CPTV file in a headless VLC with the freshly built plugin and check
# that frames come out the other end.
#   usage: smoke-test.sh <plugin.so|dylib> <file.cptv> [vlc-binary]
set -eu
PLUGIN="$1"
FILE="$2"
VLC="${3:-${VLC_BIN:-vlc}}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/plugins" "$TMP/out"
cp "$PLUGIN" "$TMP/plugins/"
VLC_PLUGIN_PATH="$TMP/plugins" "$VLC" -I dummy --no-audio --vout=dummy \
    --video-filter=scene --scene-path="$TMP/out" --scene-ratio=10 \
    --scene-prefix=frame --play-and-exit -vv "$FILE" >"$TMP/log" 2>&1 || true
if ! grep -q 'using demux module "cptv"' "$TMP/log"; then
    echo "FAIL: VLC did not use the cptv demuxer" >&2
    grep -iE 'cptv|entry point|error' "$TMP/log" >&2 || true
    exit 1
fi
n=$(ls "$TMP/out" | wc -l | tr -d ' ')
if [ "$n" -lt 1 ]; then
    echo "FAIL: no frames rendered" >&2
    exit 1
fi
grep 'cptv demux debug: CPTV:' "$TMP/log" | sed 's/.*CPTV: /OK: /'
echo "OK: $n snapshot(s) written by VLC"
