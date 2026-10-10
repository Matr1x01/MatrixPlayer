#!/usr/bin/env bash
# Builds MatrixPlayer's release packages (AppImage, .deb and .rpm) in an
# Ubuntu container, so they run on that Ubuntu release and on newer Linux
# systems: a binary needs at least the glibc it was built against, and one
# built on a rolling distribution needs the newest there is.
#
#   scripts/build-packages.sh          Ubuntu 24.04 base: runs on glibc 2.39+
#   scripts/build-packages.sh 22.04    Ubuntu 22.04 base: runs on glibc 2.35+,
#                                      but with GStreamer 1.20, older than the
#                                      1.22 MatrixPlayer needs (see README)
#
# Needs Docker. The packages and their SHA256SUMS land in
# dist-packages/ubuntu-<release>/. Rust, npm and Tauri's tool downloads are
# cached in Docker volumes between builds.
set -euo pipefail

ubuntu=${1:-24.04}
root=$(cd "$(dirname "$0")/.." && pwd)
image=matrixplayer-build:ubuntu-$ubuntu

docker build --build-arg UBUNTU="$ubuntu" -t "$image" "$root/packaging/linux"
mkdir -p "$root/dist-packages"
docker run --rm \
    -v "$root":/src:ro \
    -v "$root/dist-packages":/out \
    -v "matrixplayer-cargo-$ubuntu":/root/.cargo/registry \
    -v "matrixplayer-target-$ubuntu":/build/src-tauri/target \
    -v matrixplayer-npm:/root/.npm \
    -v "matrixplayer-tauri-$ubuntu":/root/.cache/tauri \
    -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    "$image" bash /src/packaging/linux/build.sh
