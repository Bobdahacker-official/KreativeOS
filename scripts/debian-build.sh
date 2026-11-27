#!/usr/bin/env bash
# Run on Debian 13 amd64 after enabling the arm64 architecture in APT.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
arch=${1:-}
case "$arch" in
    amd64|arm64) ;;
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
apt-get download "${kernel%%:*}:$arch" "busybox-static:$arch" "systemd-boot-efi:$arch"
kernel_deb=(linux-image-*"_$arch.deb")
busybox_deb=(busybox-static_*"_$arch.deb")
efi_deb=(systemd-boot-efi_*"_$arch.deb")
"$root/scripts/build.sh" "$arch" "$cache/${kernel_deb[0]}" "$cache/${busybox_deb[0]}" "$cache/${efi_deb[0]}"
"$root/scripts/image.sh" "$arch"
