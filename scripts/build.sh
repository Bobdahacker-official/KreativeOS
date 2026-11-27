#!/usr/bin/env bash
set -euo pipefail
umask 022
cd "$(dirname "$0")/.."

arch=${1:-}
kernel_package=${2:-}
busybox_package=${3:-}
efi_package=${4:-}
network_packages=${5:-}
case "$arch" in
    amd64) default_cc=cc; default_objcopy=objcopy; default_objdump=objdump; serial=ttyS0; efi=x64; boot=BOOTX64.EFI;;
    arm64) default_cc=aarch64-linux-gnu-gcc; default_objcopy=aarch64-linux-gnu-objcopy; default_objdump=aarch64-linux-gnu-objdump; serial=ttyAMA0; efi=aa64; boot=BOOTAA64.EFI;;
    *) echo "Usage: scripts/build.sh amd64|arm64 KERNEL.deb BUSYBOX.deb EFI-STUB.deb NETWORK-PACKAGE-DIR" >&2; exit 2;;
esac
for package in "$kernel_package" "$busybox_package" "$efi_package"; do
    [ "$(dpkg-deb -f "$package" Architecture)" = "$arch" ] || {
        echo "Expected a $arch package: $package" >&2; exit 2;
    }
done
[ -d "$network_packages" ]
build=${KOS_BUILD_DIR:-build/$arch}
mkdir -p "$build"
temp=$(mktemp -d "$build/.stage.XXXXXX")
trap 'rm -rf "$temp"' EXIT
make BUILD="$build" CC="${CC:-$default_cc}" LDFLAGS=-static "$build/kreative-shell"
for package in "$kernel_package" "$busybox_package" "$efi_package"; do
    dpkg-deb -f "$package" Package Source Version Architecture
    printf '\n'
done > "$build/COMPONENTS.txt"
dpkg-deb -x "$kernel_package" "$temp/kernel"
dpkg-deb -x "$busybox_package" "$temp/busybox"
dpkg-deb -x "$efi_package" "$temp/efi"
kernel=$(find "$temp/kernel/boot" -name 'vmlinuz-*' -type f -print -quit)
stub=$(find "$temp/efi" -name "linux$efi.efi.stub" -type f -print -quit)
[ -n "$kernel" ] && [ -n "$stub" ]
if [ "$(od -An -tx1 -N2 "$kernel" | tr -d ' \n')" = 1f8b ]; then
    gzip -dc "$kernel" > "$build/kernel"
else
    cp "$kernel" "$build/kernel"
fi
root="$temp/root"
mkdir -p "$root"/{bin,sbin,etc,dev,proc,sys,run,tmp,data}
busybox=$(find "$temp/busybox" -name busybox -type f -executable -print -quit)
install -m755 "$busybox" "$root/bin/busybox"
install -m755 "$build/kreative-shell" "$root/bin/kreative-shell"
install -m755 boot/init "$root/init"
for applet in sh ash awk basename cat chmod chown cp cut date dd df dmesg du echo env \
              false find findfs free grep head id ip kill less ln ls mdev mkdir mktemp modprobe mount mv nslookup od ping \
              poweroff printf ps pwd readlink rm rmdir sed seq sleep sort stat sync tail tar \
              tee touch tr true udhcpc umount uname uniq uptime vi wc wget which xargs yes; do
    ln -s busybox "$root/bin/$applet"
done
printf 'root:x:0:0:root:/root:/bin/sh\nnobody:x:65534:65534:KreativeOS session:/data:/bin/sh\n' > "$root/etc/passwd"
printf 'root:x:0:\ninput:x:108:\nnogroup:x:65534:\n' > "$root/etc/group"
printf 'input/event[0-9]+ 0:108 0640\nptmx 0:0 0666\n' > "$root/etc/mdev.conf"
ln -s /run/resolv.conf "$root/etc/resolv.conf"
ln -s ../bin/busybox "$root/sbin/modprobe"
install -m755 boot/network "$root/sbin/kos-network"
install -m755 boot/dhcp "$root/sbin/kos-dhcp"
install -m755 boot/wifi-event "$root/sbin/kos-wifi-event"
install -D -m644 /usr/share/terminfo/x/xterm-256color "$root/usr/share/terminfo/x/xterm-256color"
python3 scripts/network-runtime.py "$network_packages" "$temp" "$root"
version=${kernel##*/vmlinuz-}
mods=$(find "$temp/kernel" -type d -path "*/modules/$version" -print -quit)
[ -n "$mods" ]
python3 scripts/modules.py "$mods" "$root/lib/modules/$version" boot/init
python3 scripts/initramfs.py "$root" "$build/initramfs.gz"

printf 'ID=kreativeos\nNAME=KreativeOS\nVERSION_ID=0.2\n' > "$temp/os-release"
printf 'console=tty0 console=%s,115200 quiet loglevel=3%s' "$serial" "${KOS_KERNEL_ARGS:+ $KOS_KERNEL_ARGS}" > "$temp/cmdline"
read -r image_base image_size alignment < <(
    "${OBJDUMP:-$default_objdump}" -p "$stub" |
        awk '/^ImageBase/ {base=$2} /^SizeOfImage/ {size=$2}
             /^SectionAlignment/ {align=$2} END {print base, size, align}'
)
align=$((16#$alignment))
osrel=$((16#$image_base + (16#$image_size + align - 1) / align * align))
cmdline=$((osrel + align))
linux=$((cmdline + align))
kernel_size=$(wc -c < "$build/kernel")
initrd=$((linux + (kernel_size + align - 1) / align * align))
"${OBJCOPY:-$default_objcopy}" \
   --add-section ".osrel=$temp/os-release" --change-section-vma ".osrel=$(printf '0x%x' "$osrel")" \
   --add-section ".cmdline=$temp/cmdline" --change-section-vma ".cmdline=$(printf '0x%x' "$cmdline")" \
   --add-section ".linux=$build/kernel" --change-section-vma ".linux=$(printf '0x%x' "$linux")" \
   --add-section ".initrd=$build/initramfs.gz" --change-section-vma ".initrd=$(printf '0x%x' "$initrd")" \
   "$stub" "$build/$boot"
printf '%s\n' "Built $build/$boot and $build/initramfs.gz"
