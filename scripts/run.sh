#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
OVMF_CODE="${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}"
OVMF_VARS="${OVMF_VARS:-/usr/share/OVMF/OVMF_VARS_4M.fd}"
if [ ! -f build/OVMF_VARS.fd ]; then cp "$OVMF_VARS" build/OVMF_VARS.fd; fi
exec qemu-system-x86_64 -machine pc -cpu max -m 256 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file=build/OVMF_VARS.fd \
  -drive if=virtio,format=raw,file="${BOOT_IMAGE:-build/KreativeOS.img}" \
  -drive if=ide,index=0,format=raw,file=build/KreativeOS-data.img \
  -device rtl8139,netdev=net0 -netdev user,id=net0 \
  -vga std -serial "file:${SERIAL_LOG:-build/serial.log}" "$@"
