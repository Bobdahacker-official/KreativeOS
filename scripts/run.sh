#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
arch=${1:-}
if [ "$#" -gt 0 ]; then shift; fi
resolution=${KOS_RESOLUTION:-800x600}
if [[ ! "$resolution" =~ ^[0-9]{3,4}x[0-9]{3,4}$ ]]; then
    echo 'KOS_RESOLUTION must be WIDTHxHEIGHT' >&2; exit 2
fi
case "$arch" in
    amd64)
        qemu=qemu-system-x86_64; tag=x86_64
        code=${KOS_FIRMWARE:-/usr/share/OVMF/OVMF_CODE_4M.fd}
        vars=${KOS_FIRMWARE_VARS:-/usr/share/OVMF/OVMF_VARS_4M.fd}
        machine=(-machine q35 -cpu max -vga none)
        serial=ttyS0
        ;;
    arm64)
        qemu=qemu-system-aarch64; tag=aarch64
        code=${KOS_FIRMWARE:-/usr/share/AAVMF/AAVMF_CODE.fd}
        vars=${KOS_FIRMWARE_VARS:-/usr/share/AAVMF/AAVMF_VARS.fd}
        machine=(-machine virt -cpu cortex-a72)
        serial=ttyAMA0
        ;;
    *) echo "Usage: scripts/run.sh amd64|arm64 [QEMU options]" >&2; exit 2;;
esac
build=${KOS_BUILD_DIR:-build/$arch}
image=${KOS_IMAGE:-$build/KreativeOS-$tag.img}
[[ -f "$image" && ! -L "$image" ]] || { echo "Expected a regular disk image at $image" >&2; exit 2; }
if [[ ${KOS_DIRECT:-0} == 1 ]]; then
    firmware=(-kernel "$build/kernel" -initrd "$build/initramfs.gz"
        -append "console=tty0 console=$serial,115200 quiet ${KOS_CMDLINE:-}")
else
    state=${KOS_VM_VARS:-$build/uefi-vars.fd}
    mkdir -p "$(dirname "$state")"
    [[ -f "$state" ]] || cp "$vars" "$state"
    firmware=(-drive "if=pflash,unit=0,format=raw,readonly=on,file=$code"
              -drive "if=pflash,unit=1,format=raw,file=$state")
fi
exec "$qemu" "${machine[@]}" "${firmware[@]}" -m 768 -netdev "${KOS_NETDEV:-user,id=kosnet}" -device virtio-net-pci,netdev=kosnet \
    -device "virtio-gpu-pci,xres=${resolution%x*},yres=${resolution#*x}" \
    -device virtio-keyboard-pci -device virtio-tablet-pci \
    -drive "if=none,id=kos,format=raw,file=$image" -device virtio-blk-pci,drive=kos,bootindex=1 \
    -serial "${KOS_SERIAL:-stdio}" "$@"
