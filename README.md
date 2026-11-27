# KreativeOS 0.2

KreativeOS is an independent, 64-bit mobile Linux system for DIY touch devices.
It boots to first-run setup or sign-in. The native shell uses PlutoVG to render
to the Linux framebuffer and reads touch, mouse, and keyboard input through evdev.

![KreativeOS home screen on a phone-shaped display](images/home-arm64.png)

[Tablet home screen](images/home-amd64.png) · [Notes with the touch keyboard](images/notes-arm64.png) · [Terminal](images/terminal-amd64.png) · [First-run setup](images/setup-arm64.png)

## Download and boot

| Target | Download | Boot equipment |
| --- | --- | --- |
| ARM64 | [KreativeOS-aarch64.img.gz](https://raw.githubusercontent.com/Bobdahacker-official/KreativeOS/kreativeos-mobile/images/KreativeOS-aarch64.img.gz) | 64-bit UEFI with GOP or a Linux framebuffer driver |
| x86-64 | [KreativeOS-x86_64.img.gz](https://raw.githubusercontent.com/Bobdahacker-official/KreativeOS/kreativeos-mobile/images/KreativeOS-x86_64.img.gz) | 64-bit UEFI with GOP or a Linux framebuffer driver |

Verify the files using [SHA256SUMS](images/SHA256SUMS). Each download expands to
a 256 MiB GPT disk. Write it to a spare USB drive or SD card using an image writer,
then select the drive in the firmware boot menu. Imaging replaces the contents
of the selected drive. The EFI binaries are unsigned, so choose a firmware
profile that permits unsigned EFI boot. Each account keeps Notes, appearance, and home layout on the ext4 partition.
The operating system runs from RAM.

On first boot, choose the device owner's username and password. To add another
account, sign out, choose **Add user**, and enter the owner password alongside the
new login. Each shell session runs with that user's Linux UID and private home
directory. The first owner can also open documents copied from a mounted 0.1 data
partition. Writing an image to a drive replaces its partitions, so preserve
existing files before writing the new image.

### Raspberry Pi

On a Raspberry Pi with 64-bit Raspberry Pi OS Lite, boot the board with its
display and touch driver enabled and check that `/dev/fb0` exists. Copy
[the ARM64 shell](https://raw.githubusercontent.com/Bobdahacker-official/KreativeOS/kreativeos-mobile/images/kreative-shell-aarch64)
onto the Pi and run this from a checkout of this branch.

```sh
sudo bash scripts/install-pi.sh ./kreative-shell-aarch64
```

The installer enables a service that opens the KreativeOS shell at boot and
stores Notes under `/var/lib/kreativeos`. This mode uses the host service account
and Raspberry Pi OS networking. Use SSH or an alternate console for system
administration. The service reads `/etc/default/kreativeos`. Set
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
keyboard. Swipe horizontally between home pages. Swipe up on the home screen to open all apps. Swipe up from the bottom of an app to see recent apps. Tap **Arrange**, then select two tiles
to swap them across either page. The first two tiles become the bottom dock
shortcuts. Drag vertically or use the mouse wheel inside scrollable apps.
The launcher reflows between two and four columns. Linear framebuffer
formats with 16, 24 and 32 bits per pixel are supported. Touch input uses
single-touch absolute events or the first slot of a Linux type-B multitouch device.

## Apps

- **Notes** creates, edits and reopens plain-text documents. Edits autosave to
  ext4 or to the Pi data directory. Use the touch keyboard, a physical
  keyboard, or both. Tap the body to place the caret. Delete a note from its toolbar and confirm.
- **Terminal** opens an interactive Linux PTY under the signed-in account. It
  supports ANSI colors, cursor movement, scrollback, command history, foreground
  programs, and terminal resizing. Tap the prompt for the on-screen keyboard.
  **Ctrl+C** interrupts the foreground program. The touch keyboard also has a
  **Ctrl** key for other shortcuts. BusyBox provides tools such as
  `vi`, `ip`, `wget`, and `nslookup`.
- **Files** lists plain-text notes and opens them in Notes.
- **Calculator** evaluates parenthesized arithmetic and decimals.
- **Clock** shows local time and includes a stopwatch.
- **Network** shows active IPv4. Ethernet uses DHCP automatically. Tap **Scan**
  to scan Wi-Fi, select an access point, then enter its password when requested.
  The standalone image supports open and WPA2 Personal networks and
  saves a successful association for the next boot. Hardware coverage depends on the bundled Linux drivers and firmware.
- **Settings** changes appearance, shows device and session information, and
  signs out or shuts down the standalone image.

## Build and verify

From a Debian 13 x86-64 build host with the `non-free-firmware` APT component enabled:

```sh
sudo dpkg --add-architecture arm64
sudo apt-get update
sudo apt-get install git make gcc gcc-aarch64-linux-gnu libc6-dev libc6-dev-arm64-cross \
  python3 python3-pil fonts-dejavu-core libcrypt-dev libvterm-dev ncurses-base \
  binutils binutils-aarch64-linux-gnu \
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
firmware, creates an account, exercises graphical sign-in and sign-out, saves
a note, proves Terminal has a PTY, and downloads an HTTP response across virtual
Ethernet. It checks the saved files and Linux UID on ext4. The x86-64 test
also creates a second account and checks home-directory isolation.

See [design research and technical references](docs/design-research.md).

The KreativeOS source is MIT-licensed. Debian supplies the Linux kernel,
BusyBox, glibc and the systemd UEFI stub. [Component versions](images/COMPONENTS.md)
identify the Debian binary builds and their corresponding source packages.
DejaVu font attribution is in [FONT-LICENSE](FONT-LICENSE). Other notices are in
[licenses](licenses).
