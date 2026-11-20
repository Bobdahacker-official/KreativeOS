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
datafile="${KREATIVE_DATA_IMAGE:-build/KreativeOS-exfat.img}"
bootfile="${BOOT_IMAGE:-build/KreativeOS.img}"
for image in "$bootfile" "$datafile"; do
  if [[ "$image" != *.img || ! -f "$image" || -L "$image" ]]; then
    echo "Use a regular .img file for guest storage: $image" >&2
    exit 2
  fi
done
case "${STORAGE:-nvme}" in
  ide) data=(-drive "if=ide,index=0,format=raw,file=$datafile") ;;
  sata) data=(-device ich9-ahci,id=kossata -drive "if=none,id=kosdata,format=raw,file=$datafile"
    -device ide-hd,drive=kosdata,bus=kossata.0) ;;
  nvme) data=(-drive "if=none,id=kosdata,format=raw,file=$datafile"
    -device nvme,drive=kosdata,serial=KOSDATA001) ;;
  usb) data=(-drive "if=none,id=kosdata,format=raw,file=$datafile"
    -device usb-storage,bus=kosusb.0,port=3,drive=kosdata) ;;
  *) echo 'Set STORAGE to ide, sata, nvme or usb' >&2; exit 2 ;;
esac
usb=()
if [ "${STORAGE:-nvme}" = usb ] || [ "${USB_INPUT:-0}" = 1 ]; then
  usb=(-device qemu-xhci,id=kosusb)
fi
if [ "${USB_INPUT:-0}" = 1 ]; then
  usb+=(-device usb-kbd,bus=kosusb.0,port=1 -device usb-mouse,bus=kosusb.0,port=2)
fi
exec qemu-system-x86_64 -machine pc -cpu max -m 256 \
  -drive if=pflash,format=raw,readonly=on,file="$OVMF_CODE" \
  -drive if=pflash,format=raw,file=build/OVMF_VARS.fd \
  -drive if=none,id=kos,format=raw,file="$bootfile" \
  -device virtio-blk-pci,drive=kos,bootindex=1 \
  "${usb[@]}" "${data[@]}" \
  -device "${NIC_MODEL:-rtl8139},netdev=net0" -netdev user,id=net0 \
  -vga std -serial "file:${SERIAL_LOG:-build/serial.log}" "${serial2[@]}" "${audio[@]}" "$@"
