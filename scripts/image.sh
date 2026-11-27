#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
arch=${1:-}
case "$arch" in
    amd64) efi=BOOTX64.EFI; tag=x86_64;;
    arm64) efi=BOOTAA64.EFI; tag=aarch64;;
    *) echo "Usage: scripts/image.sh amd64|arm64" >&2; exit 2;;
esac
build=${KOS_BUILD_DIR:-build/$arch}
efi_source="$build/$efi"
image="$build/KreativeOS-$tag.img"
[ -s "$efi_source" ]
temp=$(mktemp -d "$build/.disk.XXXXXX")
trap 'rm -rf "$temp"' EXIT

# The first 80 MiB partition is the EFI System Partition. The remaining
# partition holds user files independently of the in-memory operating system.
truncate -s 80M "$temp/esp"
mkfs.vfat -F 32 -n KREATIVEOS "$temp/esp"
mmd -i "$temp/esp" ::/EFI ::/EFI/BOOT
mcopy -i "$temp/esp" "$efi_source" "::/EFI/BOOT/$efi"
truncate -s 174M "$temp/data"
mke2fs -q -F -t ext4 -m 0 -L KOSDATA "$temp/data"
truncate -s 256M "$temp/image"
sfdisk --no-reread --no-tell-kernel "$temp/image" >/dev/null <<'TABLE'
label: gpt
unit: sectors

start=2048, size=163840, type=uefi, name="KreativeOS Boot"
start=165888, size=356352, type=linux, name="KreativeOS Data"
TABLE
dd if="$temp/esp" of="$temp/image" bs=1M seek=1 conv=notrunc,sparse status=none
dd if="$temp/data" of="$temp/image" bs=1M seek=81 conv=notrunc,sparse status=none
mv "$temp/image" "$image"
gzip -nc "$image" > "$image.gz.tmp"
mv "$image.gz.tmp" "$image.gz"
(cd "$build" && sha256sum "KreativeOS-$tag.img.gz" > SHA256SUMS)
