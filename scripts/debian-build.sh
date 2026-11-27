#!/usr/bin/env bash
# Run on Debian 13 amd64 after enabling the arm64 architecture in APT.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
arch=${1:-}
case "$arch" in
    amd64) multiarch=x86_64-linux-gnu;;
    arm64) multiarch=aarch64-linux-gnu;;
    *) echo "Usage: scripts/debian-build.sh amd64|arm64" >&2; exit 2;;
esac
cache=$(mktemp -d)
trap 'rm -rf "$cache"' EXIT
cd "$cache"
kernel=$(apt-cache depends "linux-image-$arch:$arch" | awk '$1 == "Depends:" && $2 ~ /^linux-image-[0-9]/ {print $2; exit}')
if [ -z "$kernel" ]; then
    echo "No Debian kernel package found for $arch. Check the configured APT architectures." >&2
    exit 1
fi
apt-get download "${kernel%%:*}:$arch" "busybox-static:$arch" "systemd-boot-efi:$arch" \
    "libcrypt-dev:$arch" "libvterm-dev:$arch"
mkdir -p "$cache/network" "$cache/link"
for deb in libcrypt-dev_*.deb libvterm-dev_*.deb; do dpkg-deb -x "$deb" "$cache/link"; done
(
    cd "$cache/network"
    deps=(wpasupplicant iw libc6 libnl-3-200 libnl-genl-3-200 libnl-route-3-200
          libpcsclite1 libssl3t64 libdbus-1-3 libreadline8t64 libtinfo6 libsystemd0
          libcap2 libgcc-s1 liblzma5 libzstd1 libgcrypt20 libgpg-error0 zlib1g
          liblz4-1 libseccomp2 libudev1)
    apt-get download "${deps[@]/%/:$arch}"
    for firmware in wireless-regdb firmware-iwlwifi firmware-brcm80211 firmware-realtek; do
        apt-get download "$firmware"
    done
)
export VTERM_CPPFLAGS="-I$cache/link/usr/include"
export CRYPT_CPPFLAGS="-I$cache/link/usr/include"
export VTERM_LIBS="$cache/link/usr/lib/$multiarch/libvterm.a"
export CRYPT_LIBS="$cache/link/usr/lib/$multiarch/libcrypt.a"
kernel_deb=(linux-image-*"_$arch.deb")
busybox_deb=(busybox-static_*"_$arch.deb")
efi_deb=(systemd-boot-efi_*"_$arch.deb")
"$root/scripts/build.sh" "$arch" "$cache/${kernel_deb[0]}" "$cache/${busybox_deb[0]}" "$cache/${efi_deb[0]}" "$cache/network"
"$root/scripts/image.sh" "$arch"
