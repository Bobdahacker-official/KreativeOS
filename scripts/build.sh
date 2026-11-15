#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --target x86_64-unknown-uefi
mkdir -p build/esp/EFI/BOOT
cp target/x86_64-unknown-uefi/release/kreativeos.efi build/esp/EFI/BOOT/BOOTX64.EFI
python3 scripts/image.py
sha256sum build/KreativeOS.img build/esp/EFI/BOOT/BOOTX64.EFI > build/SHA256SUMS
