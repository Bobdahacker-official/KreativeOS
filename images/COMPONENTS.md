# Build components

| Component | x86-64 binary version | ARM64 binary version | Corresponding source |
| --- | --- | --- | --- |
| Debian Linux kernel and modules | 6.12.117-1 | 6.12.117-1 | [Linux](https://sources.debian.org/src/linux/6.12.117-1/) |
| BusyBox static | 1:1.37.0-6+b9 | 1:1.37.0-6+b9 | [BusyBox](https://sources.debian.org/src/busybox/1%3A1.37.0-6/) |
| systemd UEFI stub | 257.13-1~deb13u1 | 257.13-1~deb13u1 | [systemd](https://sources.debian.org/src/systemd/257.13-1~deb13u1/) |
| glibc linked into the native shell | 2.41-12+deb13u4 | 2.41-11cross1 | [glibc x86-64](https://sources.debian.org/src/glibc/2.41-12%2Bdeb13u4/), [glibc ARM64](https://sources.debian.org/src/glibc/2.41-11/) |
| PlutoVG | 557a81886f08 | 557a81886f08 | [PlutoVG](https://github.com/sammycage/plutovg/tree/557a81886f082cd57532cab0a9880e8393ecf52c) |
| libvterm | 0.3.3-3+b1 | 0.3.3-3+b1 | [libvterm](https://sources.debian.org/src/libvterm/0.3.3-3/) |
| libxcrypt | 1:4.4.38-1 | 1:4.4.38-1 | [libxcrypt](https://sources.debian.org/src/libxcrypt/1%3A4.4.38-1/) |
| WPA Supplicant | 2:2.10-24 | 2:2.10-24 | [wpa](https://sources.debian.org/src/wpa/2%3A2.10-24/) |
| Wireless regulatory database | 2026.09.03-1~deb13u1 | 2026.09.03-1~deb13u1 | [wireless-regdb](https://sources.debian.org/src/wireless-regdb/) |
| Selected Intel, Broadcom, and Realtek firmware | 20250410-2 | 20250410-2 | [firmware-nonfree](https://sources.debian.org/src/firmware-nonfree/20250410-2/) |

The corresponding KreativeOS source and build scripts are in this branch.
The boot artifacts were exercised with QEMU 10.0.13 and EDK2
2025.02-8+deb13u1. The screenshots came from the ARM64 and x86-64 UEFI guests.

Extended component notices are in [licenses](../licenses).
