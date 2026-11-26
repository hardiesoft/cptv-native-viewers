#!/bin/sh
# deb/rpm post-install and post-remove: refresh VLC's plugin cache and the
# MIME and desktop databases. Every step is optional.
for dir in /usr/lib/*/vlc /usr/lib64/vlc /usr/lib/vlc; do
    if [ -x "$dir/vlc-cache-gen" ]; then
        "$dir/vlc-cache-gen" "$dir/plugins" >/dev/null 2>&1 || true
    fi
done
if command -v update-mime-database >/dev/null 2>&1; then
    update-mime-database /usr/share/mime >/dev/null 2>&1 || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q /usr/share/applications >/dev/null 2>&1 || true
fi
exit 0
