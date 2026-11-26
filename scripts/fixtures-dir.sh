#!/bin/sh
# Print the directory holding cptv-tools' sample CPTV files, from Cargo's
# checkout of the `codec` dependency (pinned in Cargo.toml).
set -eu
cd "$(dirname "$0")/.."
manifest=$(${CARGO:-cargo} metadata --format-version 1 --offline 2>/dev/null \
    | grep -o '"manifest_path":"[^"]*/cptv-codec-rs/Cargo.toml"' | head -n1 \
    | sed -e 's/^"manifest_path":"//' -e 's/"$//')
if [ -z "$manifest" ]; then
    echo "cannot find the cptv-tools checkout; run 'cargo fetch' first" >&2
    exit 1
fi
echo "$(dirname "$manifest")/tests/fixtures"
