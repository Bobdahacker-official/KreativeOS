#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $# != 1 || $EUID != 0 || $(uname -m) != aarch64 ]]; then
    echo 'Usage on a 64-bit Raspberry Pi OS Lite system: sudo scripts/install-pi.sh AARCH64-SHELL' >&2
    exit 2
fi
if [[ ! -c /dev/fb0 ]]; then
    echo 'Connect a display and enable its framebuffer driver before installing.' >&2
    exit 2
fi
shell=$1
[[ -f $shell ]]
file -b "$shell" | grep -q 'ELF 64-bit.*ARM aarch64' || {
    echo 'Use the ARM64 kreative-shell built by scripts/build.sh.' >&2
    exit 2
}
getent passwd kreativeos >/dev/null || useradd --system --user-group \
    --home-dir /var/lib/kreativeos --shell /usr/sbin/nologin kreativeos
install -D -m755 "$shell" /usr/local/lib/kreativeos/kreative-shell
install -D -m644 boot/kreativeos.service /etc/systemd/system/kreativeos.service
systemctl daemon-reload
systemctl enable --now kreativeos.service
