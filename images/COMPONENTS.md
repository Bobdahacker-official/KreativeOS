# Build components

| Component | x86-64 binary version | ARM64 binary version | Corresponding source |
| --- | --- | --- | --- |
| Debian Linux kernel and modules | 6.12.117-1 | 6.12.117-1 | [Linux](https://sources.debian.org/src/linux/6.12.117-1/) |
| BusyBox static | 1:1.37.0-6+b9 | 1:1.37.0-6+b9 | [BusyBox](https://sources.debian.org/src/busybox/1%3A1.37.0-6/) |
| systemd UEFI stub | 257.13-1~deb13u1 | 257.13-1~deb13u1 | [systemd](https://sources.debian.org/src/systemd/257.13-1~deb13u1/) |
| glibc linked into the native shell | 2.41-12+deb13u4 | 2.41-11cross1 | [glibc x86-64](https://sources.debian.org/src/glibc/2.41-12%2Bdeb13u4/), [glibc ARM64](https://sources.debian.org/src/glibc/2.41-11/) |

The corresponding KreativeOS source and build scripts are in this branch.
The boot artifacts were exercised with QEMU 10.0.13 and EDK2
2025.02-8+deb13u1. The screenshots came from the ARM64 and x86-64 UEFI guests.
