#!/usr/bin/env bash
# Builds MatrixPlayer's AppImage in an Ubuntu container, so it runs on that
# Ubuntu release and on newer Linux systems: an AppImage needs at least the
# glibc it was built against, and one built on a rolling distribution needs
# the newest there is.
#
#   scripts/build-appimage.sh          Ubuntu 22.04 base: runs on glibc 2.35+
#   scripts/build-appimage.sh 24.04    Ubuntu 24.04 base: runs on glibc 2.39+
#
# Needs Docker. The AppImage lands in dist-appimage/ubuntu-<release>/. Rust,
# npm and Tauri's tool downloads are cached in Docker volumes between builds.
set -euo pipefail

ubuntu=${1:-22.04}
root=$(cd "$(dirname "$0")/.." && pwd)
image=matrixplayer-appimage:ubuntu-$ubuntu

docker build --build-arg UBUNTU="$ubuntu" -t "$image" "$root/packaging/appimage"
mkdir -p "$root/dist-appimage"
docker run --rm \
    -v "$root":/src:ro \
    -v "$root/dist-appimage":/out \
    -v "matrixplayer-cargo-$ubuntu":/root/.cargo/registry \
    -v "matrixplayer-target-$ubuntu":/build/src-tauri/target \
    -v matrixplayer-npm:/root/.npm \
    -v "matrixplayer-tauri-$ubuntu":/root/.cache/tauri \
    -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    "$image" bash /src/packaging/appimage/build.sh
