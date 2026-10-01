#!/usr/bin/env bash
# Builds the AppImage inside the build container (see
# scripts/build-appimage.sh): the working tree is mounted read-only at /src
# and copied, so the host's node_modules and target folder are never touched.
set -euo pipefail

rsync -a --delete \
    --exclude .git --exclude node_modules --exclude dist --exclude dist-appimage \
    --exclude src-tauri/target \
    /src/ /build/
cd /build
npm ci --no-audit --no-fund

# Only the GStreamer plugins in plugins.txt go into the AppImage: linuxdeploy's
# GStreamer plugin bundles whatever this folder holds, and their libraries.
system_plugins=$(pkg-config --variable=pluginsdir gstreamer-1.0)
export GSTREAMER_PLUGINS_DIR=/tmp/gstreamer-plugins
rm -rf "$GSTREAMER_PLUGINS_DIR" && mkdir -p "$GSTREAMER_PLUGINS_DIR"
while read -r name _; do
    [[ -z $name || $name == \#* ]] && continue
    plugin="$system_plugins/libgst$name.so"
    if [[ -f $plugin ]]; then
        ln -s "$plugin" "$GSTREAMER_PLUGINS_DIR/"
    else
        echo "warning: no GStreamer plugin $name on this system; the AppImage goes without it" >&2
    fi
done < packaging/appimage/plugins.txt

npx tauri build --bundles appimage

out=/out/ubuntu-$(. /etc/os-release && echo "$VERSION_ID")
mkdir -p "$out"
cp src-tauri/target/release/bundle/appimage/*.AppImage "$out/"
chown -R "${HOST_UID:-0}:${HOST_GID:-0}" /out
ls -l "$out"
