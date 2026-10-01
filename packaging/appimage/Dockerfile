# The build environment for MatrixPlayer's AppImage (see
# scripts/build-appimage.sh). An AppImage runs only where the glibc is at
# least as new as the one it was built against, so it is built on an older
# Ubuntu: 22.04 (glibc 2.35) by default, or 24.04 (glibc 2.39).
ARG UBUNTU=22.04
FROM ubuntu:${UBUNTU}

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates curl file git pkg-config rsync wget xdg-utils \
        libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
        libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
        gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
        gstreamer1.0-plugins-ugly gstreamer1.0-libav gstreamer1.0-tools \
        gstreamer1.0-pipewire gstreamer1.0-pulseaudio gstreamer1.0-alsa \
    && rm -rf /var/lib/apt/lists/*

# Node.js 22 (Vite 8 needs 20.19 or later) and Rust stable (edition 2024).
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
ENV PATH=/root/.cargo/bin:$PATH
# linuxdeploy and appimagetool are AppImages themselves; there is no FUSE in a
# container, so they unpack and run instead.
ENV APPIMAGE_EXTRACT_AND_RUN=1

WORKDIR /build
