#!/bin/sh
# Install nfpm (Linux package builder) into /usr/local/bin, checking the
# download against the release's published checksums.
#   usage: install-nfpm.sh <version, e.g. 2.48.0>
set -eu
V="$1"
case "$(uname -m)" in
    x86_64) A=x86_64 ;;
    aarch64|arm64) A=arm64 ;;
    *) echo "unsupported arch $(uname -m)" >&2; exit 1 ;;
esac
TAR="nfpm_${V}_Linux_$A.tar.gz"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
cd "$TMP"
curl -sfLO "https://github.com/goreleaser/nfpm/releases/download/v$V/$TAR"
curl -sfLO "https://github.com/goreleaser/nfpm/releases/download/v$V/checksums.txt"
grep " $TAR\$" checksums.txt | sha256sum -c -
tar -xzf "$TAR" nfpm
SUDO=""; [ "$(id -u)" = 0 ] || SUDO=sudo
$SUDO install -m 0755 nfpm /usr/local/bin/nfpm
nfpm --version | head -n1
