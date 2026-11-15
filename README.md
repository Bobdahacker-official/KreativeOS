# KreativeOS

KreativeOS is an independent x86-64 operating system written from scratch in Rust. It boots through UEFI, exits firmware boot services, and runs its own graphical desktop, input drivers, disk storage and Ethernet stack. It does not use a Linux kernel, a host operating system, or external Rust crates.

## Build and boot

Install stable Rust with the `x86_64-unknown-uefi` target, and Python 3:

```sh
rustup target add x86_64-unknown-uefi
bash scripts/build.sh
```

The build produces `build/KreativeOS.img`, a 64 MiB partitioned UEFI boot disk, `build/esp/EFI/BOOT/BOOTX64.EFI`, and a separate 8 MiB data disk. An existing data disk is preserved on rebuild.

For QEMU, install `qemu-system-x86` and `ovmf`, then run:

```sh
bash scripts/run.sh
```

Set `OVMF_CODE` and `OVMF_VARS` if your firmware files use different paths. The supplied machine uses 256 MiB RAM, standard VGA, PS/2 input, RTL8139 Ethernet, a VirtIO boot disk (read by UEFI), and an IDE primary-master data disk. The operating system contains no Linux components; Linux, macOS or Windows can be used as a build host.

GitHub Actions compiles the Rust binary, creates the disk image, boots it once and uploads the images, serial boot log and desktop screenshot as `KreativeOS-x86_64`.

## Desktop

- Custom framebuffer composition, gradient wallpaper, window shadows, taskbar, launcher, focus, dragging, minimizing and closing.
- PS/2 keyboard and three-byte PS/2 mouse packets.
- Files and Notes with a persistent, checksummed, two-snapshot KFS volume.
- Terminal with file operations, app launching, network status, calculation and reboot.
- HTTP browser with DNS resolution, TCP requests, HTML-to-text rendering, local HTML documents and scrolling.
- Calculator with checked integer arithmetic, operator precedence and parentheses.
- Paint with a color palette, continuous strokes and clear canvas.
- Snake with food, scoring, self-collision, wall collision and restart.
- Two-player Tic Tac Toe with turn management, win detection and restart.
- Local KAPP1 document packages, installation persistence and a launcher entry.
- Network status with real Ethernet counters, MAC address and PCI Wi-Fi device discovery.
- Settings with three accent colors, save-all and restart.

F1–F10 open Files through Settings. Escape toggles the launcher. Tab switches open windows. Click a title bar to drag, `_` to minimize, or `X` to close. In Notes, click Save. In Browser, type an address and press Enter; use Up/Down to scroll. Snake uses arrow keys and Space to restart.

## Networking and hardware

The supplied configuration uses QEMU user networking: IPv4 `10.0.2.15/24`, gateway `10.0.2.2`, DNS `10.0.2.3`. The Ethernet driver targets PCI RTL8139 with 32-bit DMA allocations. The stack implements Ethernet, ARP, IPv4 validation, UDP DNS lookup and a single bounded TCP/HTTP connection with retransmission and timeout handling.

Open `http://example.com/` in Browser or `file:Readme.html` for the bundled local document. Browser output is text, capped at 12 KiB. HTTP uses port 80 and HTTP/1.0 connection-close semantics. HTTPS/TLS, JavaScript, CSS layout, DHCP and Wi-Fi radio association are not implemented. Wi-Fi devices are counted through PCI discovery; the Network app reports driver availability accurately.

The kernel is a single-address-space system with a cooperative app event loop and polled drivers. It currently uses firmware-established CPU mappings. USB input, audio, process isolation, SMP and general-purpose native executable loading are not implemented. Paint content stays in memory. KAPP packages contain document apps, not native machine code.

## Files and packages

KFS has 24 file entries, 32-byte ASCII names and 2 KiB per file. It stores snapshots in sectors 2048–2247 of the dedicated primary-master ATA disk, committing the header last. Unrecognized nonblank KFS regions are not formatted. A missing disk leaves files available in memory, and Save reports that state.

Terminal examples:

```text
help
ls
edit Ideas.txt
cat Welcome.txt
calc (12 + 8) * 3
browse http://example.com/
install Guide.kapp
open Snake
save
```

The included `Guide.kapp` is installed as `Guide.app`. Its KAPP1 header contains a name, version and document body. Installation validates the format and writes the installed document to KFS.

See [architecture](docs/architecture.md), [UI references](docs/design.md) and [verification](docs/verification.md).
