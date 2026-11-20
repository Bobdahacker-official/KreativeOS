#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
OVMF_CODE="${OVMF_CODE:-/usr/share/OVMF/OVMF_CODE_4M.fd}"
OVMF_VARS="${OVMF_VARS:-/usr/share/OVMF/OVMF_VARS_4M.fd}"
if [ ! -f build/OVMF_VARS.fd ]; then cp "$OVMF_VARS" build/OVMF_VARS.fd; fi
audio=(-device AC97)
if [ -n "${AUDIO_SPEC:-}" ]; then
  audio=(-audiodev "$AUDIO_SPEC,id=kos_audio" -device AC97,audiodev=kos_audio)
fi
serial2=()
if [ -n "${ESP_AT_SERIAL:-}" ]; then
  serial2=(-serial "$ESP_AT_SERIAL")
fi
case "${STORAGE:-ide}" in
  ide) data=(-drive "if=ide,index=0,format=raw,file=${KREATIVE_DATA_IMAGE:-build/KreativeOS-data.img}") ;;
  sata) data=(-device ich9-ahci,id=kossata -drive "if=none,id=kosdata,format=raw,file=${KREATIVE_DATA_IMAGE:-build/KreativeOS-sata.img}"
    -device ide-hd,drive=kosdata,bus=kossata.0) ;;
  nvme) data=(-drive "if=none,id=kosdata,format=raw,file=${KREATIVE_DATA_IMAGE:-build/KreativeOS-nvme.img}"
    -device nvme,drive=kosdata,serial=KOSDATA001) ;;
  *) echo 'Set STORAGE to ide, sata or nvme' >&2; exit 2 ;;
esac
exec qemu-system-x86_64 -machine pc -cpu max -m 256 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file=build/OVMF_VARS.fd \
  -drive if=none,id=kos,format=raw,file="${BOOT_IMAGE:-build/KreativeOS.img}" \
  -device virtio-blk-pci,drive=kos,bootindex=1 \
  "${data[@]}" \
  -device "${NIC_MODEL:-rtl8139},netdev=net0" -netdev user,id=net0 \
  -vga std -serial "file:${SERIAL_LOG:-build/serial.log}" "${serial2[@]}" "${audio[@]}" "$@"
