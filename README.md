# KreativeOS

KreativeOS is an independent x86-64 operating system written from scratch in Rust. It boots through UEFI, exits firmware boot services, and runs its own graphical desktop, input drivers, disk storage and Ethernet stack. The kernel runs directly on x86-64. The browser uses Rust libraries for TLS and certificate validation.

## Download

[Current builds are available on GitHub Actions](https://github.com/Bobdahacker-official/KreativeOS/actions/workflows/build.yml). Open the latest successful `main` build and download its `KreativeOS-x86_64` artifact. It includes the boot disk, data disk, EFI binary, and checksums.

![KreativeOS desktop](docs/desktop.png)

## Build and boot

Install stable Rust with the `x86_64-unknown-uefi` target, and Python 3:

```sh
rustup target add x86_64-unknown-uefi
bash scripts/build.sh
```

The build produces `build/KreativeOS.img`, a 64 MiB partitioned UEFI boot disk, `build/esp/EFI/BOOT/BOOTX64.EFI`, and `build/KreativeOS-exfat.img` for persistent data. Existing data images are preserved on rebuild.

For QEMU, install `qemu-system-x86` and `ovmf`, then run:

```sh
bash scripts/run.sh
```

On Windows, install stable Rust, Python 3 and QEMU, then run from PowerShell:

```powershell
rustup target add x86_64-unknown-uefi
.\scripts\build.ps1
.\scripts\run.ps1 -Headless
```

The headless launch writes serial output to `build/serial.log`. Omit `-Headless` to open the desktop. Set `KREATIVE_PYTHON` or `KREATIVE_QEMU` to the executable path if automatic discovery fails.

Use `.\scripts\run.ps1 -Storage SATA -Headless` on Windows or `STORAGE=sata bash scripts/run.sh -display none` on Linux to use the native AHCI driver. Both commands attach the same type of data image used by the bare-metal driver.
Use `.\scripts\run.ps1 -Storage NVME -Headless` or `STORAGE=nvme bash scripts/run.sh -display none` for the native NVMe driver.

### ASRock A520M-HVS USB profile

```powershell
.\scripts\build.ps1 -PrepareUsb
.\scripts\run-a520.ps1 -Headless
```

The first command creates `build/KreativeOS-A520-usb.img` with a FAT EFI boot partition and a separate exFAT data partition. It refuses to overwrite an existing USB image. Set `KREATIVE_USB_IMAGE` to a new image filename to build another copy. The second command boots that image in QEMU with xHCI storage, keyboard and mouse. Omit `-Headless` for the desktop. These commands use local image files and do not write to any physical drive or change Windows boot settings.

The image also provides the removable UEFI boot path `EFI/BOOT/BOOTX64.EFI`. For physical boot, use dedicated removable media and the firmware's one-time boot menu. Connect the keyboard and mouse to motherboard ports before boot. See [A520M-HVS compatibility notes](docs/a520m-hvs.md).

Set `OVMF_CODE` and `OVMF_VARS` if your firmware files use different paths. The supplied machine uses 256 MiB RAM, standard VGA, PS/2 input, RTL8139 Ethernet, a VirtIO boot disk (read by UEFI), and an IDE primary-master data disk. The operating system contains no Linux components; Linux, macOS or Windows can be used as a build host.

GitHub Actions compiles the Rust binary, verifies a headless QEMU boot, and exercises the browser against local HTTP and TLS servers. It uploads disk images, checksums, and serial logs as `KreativeOS-x86_64`.

## Desktop

- tiny-skia vector icons, a composited pointer, antialiased Open Sans, an aurora wallpaper, and a three-column app launcher with workspace search. Click 1–4 at the top right to switch workspaces. Windows support dragging, minimizing, maximizing and closing.
- PS/2 keyboard, standard PS/2 mice, and IntelliMouse-compatible scroll wheels.
- Files and Notes with persistent exFAT files and Notes draft recovery.
- Terminal with file operations, app launching, network status, calculation and reboot.
- HTTP and HTTPS browser with DNS resolution, local HTML documents, inline images and WAV playback over HTTPS.
- Calculator with checked integer arithmetic, operator precedence and parentheses.
- Images opens PNG, JPEG and uncompressed BMP at fit or actual size. The gallery imports packs from the boot disk and firmware-mounted USB storage.
- Paint with a color palette, continuous strokes and clear canvas.
- Snake with food, scoring, self-collision, wall collision and restart.
- Two-player Tic Tac Toe with turn management, win detection and restart.
- Local KAPP1 document packages, installation persistence and a launcher entry.
- Separate Ethernet and ESP-AT Wi-Fi panels. Ethernet shows link speed, the active IPv4 configuration and remaining DHCP lease time, and saves automatic or manual profiles with a VLAN setting.
- Hardware Center shows the processor, active storage and Ethernet drivers, the mounted filesystem, and a searchable PCI inventory. Use **Save report** to export the inventory to `Hardware.txt`.
- Settings with four wallpaper presets, color modes, saved accent and magnifier preferences, live system details, and restart confirmation.
- The floating dock opens the app grid and the activity panel, which shows live memory use, file usage, network traffic and uptime.

F1–F10 open Files through Settings. Escape toggles the launcher. Use the arrow keys and Enter to open an app from the grid. Tab switches open windows. Drag the title bar to move a window. Use the controls on the right to minimize, maximize or restore, and close. In Notes, click Save or press Ctrl+S. In Browser, press Ctrl+L to clear the address, then type an address and press Enter. Use Up/Down to scroll. Snake uses arrow keys and Space to restart.

On writable exFAT, Notes keeps one private recovery checkpoint per account, including standard accounts. The checkpoint supports nested filenames and notes up to the normal file limit. A successful save clears the checkpoint for that document. If a save fails, the edit stays open. Use `discard note` in Terminal to intentionally drop the open note's unsaved changes.

## Networking and hardware

Ethernet requests an IPv4 lease automatically on boot. It renews with the original DHCP server and broadcasts a rebind request if renewal fails. Network → Ethernet can save an automatic or manual profile to the data disk. Use VLAN 0 for an untagged network and `0.0.0.0` for an unused gateway or resolver in manual settings. The QEMU user network normally assigns `10.0.2.15/24`, gateway `10.0.2.2`, and DNS `10.0.2.3`.

Ethernet supports PCI RTL8139 and Intel 82540EM, 82544GC, 82545EM, and 82574L. QEMU defaults to RTL8139. Set `NIC_MODEL=e1000` for 82540EM or `NIC_MODEL=e1000e` for 82574L. The stack implements Ethernet, ARP, IPv4, UDP DNS with CNAME resolution, and a single bounded TCP/HTTP connection with retransmission and timeouts. DNS is IPv4 only. It reports truncated UDP replies and does not retry them over TCP.

The native Realtek PCIe driver targets RTL8168H/8111H controllers with PCI ID `10ec:8168` and the H-generation MAC signature. It uses DMA queues and the same Ethernet, DHCP and DNS stack. Physical link negotiation and traffic on the RTL8168H still need hardware verification. [OpenBSD RTL8168H register reference](https://github.com/openbsd/src/blob/master/sys/dev/ic/re.c)

Open `https://example.com/` in Browser or `file:Readme.html` for the bundled local document. The browser shows page text, the first compatible HTML image and a Play sound button for the first WAV audio source. Select **Links** to open links from the current web page, and use the scroll wheel or Up/Down to see more. Direct links to PNG, JPEG, BMP and WAV also open. The browser uses HTTP/1.1 and supports redirects, chunked bodies, and single-member gzip. The response buffer and decompressed body are each capped at 768 KiB. HTTPS uses TLS 1.3 and checks the certificate chain, hostname and validity dates against the guest clock and the bundled Mozilla roots. It requires an x86 CPU with RDRAND, enabled by the supplied QEMU configuration. Plain HTTP uses Ethernet. HTTPS pages and resources stay on HTTPS when following links and redirects.

The graphical desktop uses a cooperative event loop and polled drivers. KR3 programs enter CPU ring 3 in separate address spaces. Paint content stays in memory. KAPP packages contain document apps.

## Wi-Fi and sound

Connect an ESP32 running Espressif ESP-AT to COM2 using a UART adapter appropriate for the board and the PC port. The driver uses 115200 baud, 8 data bits, no parity and one stop bit. In **Network** enter the SSID and password, then select **Connect**. The module associates and gets its own IP address. HTTPS then uses the module's TCP transport with certificate and hostname checks inside KreativeOS.

QEMU can attach an ESP-AT serial backend with `ESP_AT_SERIAL`. The integration check uses a local protocol fixture for this serial transport.

The Realtek RTL8811CU USB adapter (`0bda:c811`) has a firmware startup path that reports status under **Network** and on the serial console. Set `RTL8811CU_FIRMWARE` to `rtw8821c_fw.bin` from linux-firmware tag `20250708` when building an image. The image builder places it at `RTL8821C.BIN` in the EFI partition. The kernel checks the firmware's SHA-256, transfers it through the adapter's USB DMA queue and waits for the firmware-ready register. The driver then reads factory board data from eFuse and receives the firmware's capability report. The serial console and Hardware Center report the radio front-end option, channel plan and crystal trim. [Pinned Realtek firmware](https://kernel.googlesource.com/pub/scm/linux/kernel/git/firmware/linux-firmware/+/refs/tags/20250708/rtw88/rtw8821c_fw.bin) · [Realtek eFuse reference](https://github.com/torvalds/linux/blob/master/drivers/net/wireless/realtek/rtw88/efuse.c)

**Scan Wi-Fi** can request a native passive scan when an RTL8811CU was detected at boot and ESP-AT is disconnected. The driver configures the factory radio profile and listens for beacons on channels 1–11 at 20 MHz. The first scan loads the PHY tables. The UI lists visible SSIDs and their advertised security without transmitting probe requests. The passive scan still needs physical adapter validation. Joining a network through the native driver remains unimplemented. The PHY data is distributed under the Realtek BSD license in `RTPHY.LIC` on the EFI partition. [Realtek PHY tables](https://github.com/torvalds/linux/blob/v6.18/drivers/net/wireless/realtek/rtw88/rtw8821c_table.c)

The browser plays 16-bit PCM WAV through Intel ICH AC'97. QEMU adds the audio controller at boot. Set `AUDIO_SPEC=driver=pa` or another backend supported by your QEMU install if you want to select the host audio output explicitly.

## Import your images

Copy PNG, JPEG or uncompressed BMP files into an `imports/` directory at the repository root and rebuild the boot image. On boot, open **Images** from the taskbar. Use the arrow keys or mouse wheel to browse, and press Space to switch between fit and actual size.

To import from USB, create a pack with the included script and put `IMAGES.KIP` in the root of a FAT-formatted USB drive. Connect the drive before boot.

```sh
python3 scripts/pack_images.py photo.jpg drawing.png
```

The gallery reads up to 36 imported images from packs totaling 6 MiB. Each image can be up to 4,096 pixels on either side, with a total of 2,621,440 pixels.

## Files and packages

New volumes use exFAT 1.00. KreativeOS mounts one valid data volume, looking for USB first, then NVMe, AHCI SATA, and IDE. Ordinary exFAT volumes mount read-only until you confirm writes in first-boot setup or sign in to an existing account on that volume. Previously prepared `KOSDAT` data volumes remain writable by default. Files and Notes support nested folders and files up to 1 MiB. The guest caches up to 1,024 files within a 12 MiB cache budget. Existing KFS images remain available through `KREATIVE_DATA_IMAGE` with their original limits.

### SSDs, HDDs and data migration

Use `KreativeOS-exfat.img` as a dedicated virtual data disk. The combined USB image keeps EFI boot and data in separate partitions. AHCI, NVMe and USB BOT support 512-byte and 4 KiB logical sectors. The kernel verifies GPT checksums and partition bounds, also recognizes primary MBR partitions and whole-device exFAT, and confines filesystem writes to the selected volume. The older IDE path uses 512-byte sectors.

```sh
python scripts/exfat_image.py build/data-gpt-4kn.img --partition gpt --sector-size 4096
```

Attach a 4Kn image to a virtual disk configured with 4,096-byte logical sectors. Use `mkdir Projects` and `mkdir Projects/Notes` to create folders, then open them in Files or use `edit Projects/Notes/report.txt` in Terminal. A structurally readable volume with a dirty or media-failure flag mounts read-only and can still be browsed after sign-in. Use host tools to repair the volume before writing again.

To copy legacy files and accounts to a new SATA image while keeping the original image:

```sh
python scripts/sata_image.py build/migrated-sata.img --from-ide build/KreativeOS-data.img
```

Set `KREATIVE_DATA_IMAGE` to the new image and select the SATA launch profile. The migration script refuses to overwrite an existing output file. See [hardware research](docs/hardware.md) for the driver design and compatibility boundaries.

Add `--nvme` to that command to create a new NVMe image from the legacy IDE image. Existing marked SATA images also work with the NVMe profile. To move an existing SATA volume, make a copy of its image and point `KREATIVE_DATA_IMAGE` at the copy.

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

The included `Guide.kapp` is installed as `Guide.app`. Its KAPP1 header contains a name, version and document body. Installation validates the format and writes the installed document to the data volume.

## Develop on the host

Put UTF-8 `.txt`, `.html` and `.ksh` files or binary `.kr3` programs in a directory and point `KREATIVE_DEV_DIR` at it before building. The builder embeds up to 1 MiB of staged data in the boot image. Names can contain up to 32 printable ASCII characters. Inside the guest, open Terminal and run:

```text
dev list
dev import
run setup.ksh
browse file:Demo.html
```

`dev import` preserves existing filenames. Import requires sufficient free file slots. A `.ksh` file contains one KreativeOS terminal command per line and runs when the administrator invokes `run`.

### Accounts and native programs

1. At first boot, create the local administrator on a writable data disk. Choose a lowercase username and a password of at least 12 characters. Subsequent boots open the sign-in screen.
2. Select **Create an account** at sign-in to register a standard user. Any signed-in user can change their password or sign out under **Settings → Accounts**. The administrator can also add users and manage them from Terminal with `accounts`.
3. Name a program `alice.hello.kr3` to assign it to `alice`. Use `dev import` to install the program. The administrator can also try it with `exec alice.hello.kr3` from Terminal.
4. Sign out and sign in as `alice`. Open Terminal from the desktop, run `ls`, then run `exec hello.kr3`.

New passwords use PBKDF2-HMAC-SHA-256 with 600,000 iterations and a fresh 128-bit hardware-generated salt. Existing local account files remain readable.

Both account types open the desktop. Standard accounts have separate file and application settings, and Terminal runs their own KR3 executables. Administration, system networking changes and restart require the administrator. `exec NAME.kr3` queues a process, `ps` lists the session's processes, and `kill PID` terminates one. The single-core scheduler saves and restores CPU state on each APIC time slice and services desktop input between slices. Each process has its own address space, up to 64 KiB of executable code, a 16 KiB guarded stack, and up to 512 KiB of demand-allocated heap.

On exFAT, each standard account gets the full 32-character local filename allowance. The storage prefix is separate from that limit. When a file is replaced or removed, the exFAT driver clears the retired data clusters before making them available for reuse.

An administrator can open the built-in maintenance desktop by typing `system1` directly in Terminal. The service has no ordinary sign-in. Its ring-3 worker updates the peak heap measurement and prepares the budget for a free-space scrub. In the maintenance desktop, `service` shows the worker and scrub status, `scrub free` starts a scan of free clusters on the mounted writable exFAT volume, and `scrub cancel` stops it. `memory set N` changes the live service-owned scrub budget to 1–8 clusters per worker run. Use `lock` to return to sign-in. An account created with the old username `system1` uses `local:system1` at sign-in and retains its original role.

Build the example with NASM:

```sh
nasm -f bin examples/user/hello.asm -o alice.hello.kr3
```

KR3 version 1 takes a flat x86-64 binary with the four-byte header `4b 52 33 01`. Execution starts at the first byte after the header, mapped at `0x400000000000`. Use RIP-relative addressing. The `int 0x80` ABI uses EAX for the call number. Call 0 exits with EBX as the status. Call 1 writes up to 512 bytes from RBX with length in RSI, returning the byte count in RAX. Call 2 grows the private heap to the requested size in RBX and returns its base in RAX. A negative result indicates an error.

See [architecture](docs/architecture.md), [UI references](docs/design.md) and [verification](docs/verification.md).
