# KreativeOS

KreativeOS is an independent x86-64 operating system written from scratch in Rust. It boots through UEFI, exits firmware boot services, and runs its own graphical desktop, input drivers, disk storage and Ethernet stack. The kernel runs directly on x86-64. The browser uses Rust libraries for TLS and certificate validation.

## Download

[Download the compiled KreativeOS boot bundle](dist/KreativeOS-x86_64.zip?raw=true). It includes the boot disk, blank data disk, EFI binary, checksums, serial log and captured desktop. Extract the bundle into `build/` to use `scripts/run.sh`.

![KreativeOS desktop](docs/desktop.png)

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

GitHub Actions compiles the Rust binary, boots the disk image in QEMU, and checks HTTPS against local TLS servers. It uploads the images, serial logs and desktop screenshot as `KreativeOS-x86_64`.

## Desktop

- tiny-skia vector icons, a composited pointer, antialiased Open Sans, a jade grid wallpaper, a full-width taskbar, and a two-column app launcher. Windows support dragging, minimizing, maximizing and closing.
- PS/2 keyboard, standard PS/2 mice, and IntelliMouse-compatible scroll wheels.
- Files and Notes with a persistent, checksummed, two-snapshot KFS volume.
- Terminal with file operations, app launching, network status, calculation and reboot.
- HTTP and HTTPS browser with DNS resolution, local HTML documents, inline images and WAV playback over HTTPS.
- Calculator with checked integer arithmetic, operator precedence and parentheses.
- Images opens PNG, JPEG and uncompressed BMP at fit or actual size. The gallery imports packs from the boot disk and firmware-mounted USB storage.
- Paint with a color palette, continuous strokes and clear canvas.
- Snake with food, scoring, self-collision, wall collision and restart.
- Two-player Tic Tac Toe with turn management, win detection and restart.
- Local KAPP1 document packages, installation persistence and a launcher entry.
- Network settings for Ethernet and ESP-AT Wi-Fi.
- Settings with three accent colors, save-all and restart.

F1–F10 open Files through Settings. Escape toggles the launcher. Tab switches open windows. Drag the title bar to move a window. Use the controls on the right to minimize, maximize or restore, and close. In Notes, click Save. In Browser, press Ctrl+L to clear the address, then type an address and press Enter; use Up/Down to scroll. Snake uses arrow keys and Space to restart.

## Networking and hardware

The supplied configuration uses QEMU user networking: IPv4 `10.0.2.15/24`, gateway `10.0.2.2`, DNS `10.0.2.3`. Ethernet supports PCI RTL8139 and Intel PRO/1000 MT Desktop (82540EM). QEMU defaults to RTL8139. Set `NIC_MODEL=e1000` to select the Intel adapter. The stack implements Ethernet, ARP, IPv4 validation, UDP DNS lookup and a single bounded TCP/HTTP connection with retransmission and timeout handling.

Open `https://example.com/` in Browser or `file:Readme.html` for the bundled local document. The browser shows page text, the first compatible HTML image and a Play sound button for the first WAV audio source. Direct HTTPS links to PNG, JPEG, BMP and WAV also open. Media responses are capped at 768 KiB. HTTP uses port 80 and HTTP/1.0 connection-close semantics. HTTPS uses TLS 1.3 and checks the certificate chain, hostname and validity dates against the guest clock and the bundled Mozilla roots. It requires an x86 CPU with RDRAND, enabled by the supplied QEMU configuration. The browser supports authenticated HTTPS redirects and chunked response bodies. Plain HTTP uses the Ethernet interface.

The kernel is a single-address-space system with a cooperative app event loop and polled drivers. It currently uses firmware-established CPU mappings. USB input, process isolation, SMP and general-purpose native executable loading are not implemented. Paint content stays in memory. KAPP packages contain document apps, not native machine code.

## Wi-Fi and sound

Connect an ESP32 running Espressif ESP-AT to COM2 using a UART adapter appropriate for the board and the PC port. The driver uses 115200 baud, 8 data bits, no parity and one stop bit. In **Network** enter the SSID and password, then select **Connect**. The module associates and gets its own IP address. HTTPS then uses the module's TCP transport with certificate and hostname checks inside KreativeOS.

QEMU can attach an ESP-AT serial backend with `ESP_AT_SERIAL`. The integration check uses a local protocol fixture for this serial transport.

The browser plays 16-bit PCM WAV through Intel ICH AC'97. QEMU adds the audio controller at boot. Set `AUDIO_SPEC=driver=pa` or another backend supported by your QEMU install if you want to select the host audio output explicitly.

## Import your images

Copy PNG, JPEG or uncompressed BMP files into an `imports/` directory at the repository root and rebuild the boot image. On boot, open **Images** from the taskbar. Use the arrow keys or mouse wheel to browse, and press Space to switch between fit and actual size.

To import from USB, create a pack with the included script and put `IMAGES.KIP` in the root of a FAT-formatted USB drive. Connect the drive before boot.

```sh
python3 scripts/pack_images.py photo.jpg drawing.png
```

The gallery reads up to 36 imported images from packs totaling 6 MiB. Each image can be up to 4,096 pixels on either side, with a total of 2,621,440 pixels.

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
