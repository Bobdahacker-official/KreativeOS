# KreativeOS

KreativeOS is an independent, 64-bit mobile Linux system for DIY touch devices. It
boots straight into a native C shell that renders through the Linux framebuffer
and reads touch, mouse and keyboard input through evdev. The launcher and apps
share a small software renderer with no window server, browser runtime or desktop environment.

![KreativeOS home screen on a phone-shaped display](images/home-arm64.png)

[Tablet home screen](images/home-amd64.png) · [Notes with the touch keyboard](images/notes-arm64.png) · [Terminal](images/terminal-amd64.png)

## Download and boot

| Target | Download | Boot equipment |
| --- | --- | --- |
| ARM64 | [KreativeOS-aarch64.img.gz](https://raw.githubusercontent.com/Bobdahacker-official/KreativeOS/kreativeos-mobile/images/KreativeOS-aarch64.img.gz) | 64-bit UEFI with GOP or a Linux framebuffer driver |
| x86-64 | [KreativeOS-x86_64.img.gz](https://raw.githubusercontent.com/Bobdahacker-official/KreativeOS/kreativeos-mobile/images/KreativeOS-x86_64.img.gz) | 64-bit UEFI with GOP or a Linux framebuffer driver |

Verify the files using [SHA256SUMS](images/SHA256SUMS). Each download expands to
a 256 MiB GPT disk. Write it to a spare USB drive or SD card using an image writer,
then select the drive in the firmware boot menu. Imaging replaces the contents
of the selected drive. The EFI binaries are unsigned, so choose a firmware
profile that permits unsigned EFI boot. Notes and appearance use the ext4
partition on the same drive. The operating system itself runs from RAM.

### Raspberry Pi

On a Raspberry Pi with 64-bit Raspberry Pi OS Lite, boot the board with its
display and touch driver enabled and check that `/dev/fb0` exists. Copy
[the ARM64 shell](https://raw.githubusercontent.com/Bobdahacker-official/KreativeOS/kreativeos-mobile/images/kreative-shell-aarch64)
onto the Pi and run this from a checkout of this branch.

```sh
sudo bash scripts/install-pi.sh ./kreative-shell-aarch64
```

The installer enables a service that opens the KreativeOS shell at boot and
stores Notes under `/var/lib/kreativeos`. Use SSH or an alternate console for
system administration. The service reads `/etc/default/kreativeos`. Set
`KOS_ROTATE=90` or `KOS_SCALE=2` there for panels that need orientation or density
adjustment, then restart the service. The standalone image accepts the same
values when built with `KOS_KERNEL_ARGS='kos.rotate=90 kos.scale=2'`; use
`KOS_DIRECT=1 KOS_CMDLINE='kos.rotate=90 kos.scale=2'` for local QEMU testing.

### QEMU

On Debian, install `qemu-system-x86 ovmf` or `qemu-system-arm qemu-efi-aarch64`.
From this checkout, decompress the desired image and set `KOS_IMAGE` to its
absolute path.

```sh
gzip -dk KreativeOS-aarch64.img.gz
KOS_IMAGE="$PWD/KreativeOS-aarch64.img" KOS_VM_VARS=/tmp/kreative-arm64-uefi.fd \
  KOS_RESOLUTION=480x800 bash scripts/run.sh arm64
```

Use `amd64` for the x86-64 image. QEMU enrolls a virtual absolute tablet and
keyboard. Drag vertically to scroll, or use the mouse wheel. The shell lays
out one to three columns based on the available width. Linear framebuffer
formats with 16, 24 and 32 bits per pixel are supported. Touch input uses
single-touch absolute events or the first slot of a Linux type-B multitouch device.

## Apps

- **Notes** creates, edits and reopens plain-text documents. Edits autosave to
  ext4 or to the Pi data directory. Use the touch keyboard, a physical
  keyboard, or both. Tap the body to place the caret.
- **Terminal** runs commands through `/bin/sh` as an unprivileged user and
  captures stdout and stderr. It supports pipelines, redirection, working
  directories and command history. Tap the prompt for the soft keyboard, use
  its arrows to recall commands, and tap **Stop** to end a running command.
  The standalone image includes BusyBox command-line tools.
- **Files** lists plain-text notes and opens them in Notes.
- **Calculator** evaluates parenthesized arithmetic and decimals.
- **Clock** shows local time and includes a stopwatch.
- **Settings** changes appearance, displays resolution and CPU architecture,
  and shuts down the standalone image.

## Build and verify

From a Debian 13 x86-64 build host:

```sh
sudo dpkg --add-architecture arm64
sudo apt-get update
sudo apt-get install make gcc gcc-aarch64-linux-gnu libc6-dev libc6-dev-arm64-cross \
  python3 python3-pil fonts-dejavu-core binutils binutils-aarch64-linux-gnu \
  dosfstools mtools fdisk e2fsprogs zstd kmod qemu-system-x86 qemu-system-arm \
  ovmf qemu-efi-aarch64
make test
bash scripts/debian-build.sh amd64
bash scripts/debian-build.sh arm64
python3 scripts/smoke.py amd64
python3 scripts/smoke.py arm64
```

Builds produce `build/amd64/KreativeOS-x86_64.img.gz` and
`build/arm64/KreativeOS-aarch64.img.gz`. The QEMU check boots each disk through
firmware, drives the GUI with virtual input, saves a note, runs `uname -m`
from Terminal, and reads both files back from the image's ext4 partition.

The KreativeOS source is MIT-licensed. Debian supplies the Linux kernel,
BusyBox, glibc and the systemd UEFI stub. [Component versions](images/COMPONENTS.md)
identify the Debian binary builds and their corresponding source packages.
DejaVu font attribution is in [FONT-LICENSE](FONT-LICENSE).
