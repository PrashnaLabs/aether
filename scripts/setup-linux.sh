#!/usr/bin/env bash
# Ubuntu 24.04 / Debian 13 build dependencies. Run with sudo.
set -euo pipefail
if [[ $(id -u) -ne 0 ]]; then
  echo "Run: sudo bash scripts/setup-linux.sh" >&2
  exit 1
fi
apt-get update
apt-get install -y build-essential clang cmake pkg-config python3 curl \
  libfontconfig1-dev libfreetype6-dev libwayland-dev libxkbcommon-x11-dev \
  libx11-xcb-dev libxcb1-dev libxcb-composite0-dev libxcb-damage0-dev \
  libxcb-randr0-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxcb-sync-dev libxcb-xinput-dev libxcb-xkb-dev libvulkan-dev \
  libgtk-3-dev libwebkit2gtk-4.1-dev libssl-dev libzstd-dev libasound2-dev \
  libgit2-dev libdbus-1-dev libudev-dev libclang-dev \
  dpkg-dev desktop-file-utils libvulkan1 mesa-vulkan-drivers
