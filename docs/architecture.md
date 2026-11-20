# Architecture

`efi_main` is a PE32+ UEFI entry point. The loader locates GOP, selects 1024x768 when supported, allocates a composition buffer and low-memory DMA pages, captures the firmware memory map and exits boot services. Firmware code is never called afterward.

The kernel masks legacy PIC interrupts and runs a polling loop. PIT channel 2 calibrates TSC timing. Input reads the i8042 controller with mouse packet synchronization and keyboard scan-set-1 translation. Graphics compose into a RAM buffer then copy to GOP memory with RGB/BGR conversion. The RTC supplies taskbar time.

RTL8139 discovery uses PCI configuration mechanism 1. Receive and transmit DMA use a reserved allocation below 4 GiB. Frames pass through length, IPv4 and TCP checksum validation before reaching the single HTTP client. DNS parsing bounds every name and record. Ethernet hardware, transport state and browser text are separated into driver and application state.

KFS uses ATA PIO on a dedicated disk. Two 100-sector snapshots alternate. Each has a magic, generation, checksum and 24 fixed-size file records. A save invalidates the destination header, flushes all payload sectors and publishes the new header last. Mount validates payload checksums before selecting the latest valid snapshot. Disk errors are surfaced to the desktop; they do not silently report persistence.

Applications share one cooperative kernel event loop. Window order is independent of stable taskbar slots. Package installation creates persistent document applications. No app is described as sandboxed or ring-3 isolated.

Source map:

| File | Responsibility |
|---|---|
| `boot.rs` | UEFI ABI and boot transition |
| `hardware.rs` | Port I/O, PCI, PS/2, serial, timer, RTC |
| `graphics.rs` | Pixel composition, glyphs, lines, text |
| `storage.rs` | ATA PIO, KFS mount and commit |
| `net.rs` | RTL8139, ARP, DNS, TCP, HTTP and HTML text |
| `desktop.rs` | Window manager, apps, package installation |
| `text.rs` | Fixed-capacity strings |
| `scripts/image.py` | FAT16 boot image creation |

The image gallery loads bounded image packs using UEFI Simple File System before the kernel exits boot services. The native renderer shows PNG, JPEG and BMP in fit and actual-size modes. The Intel 82540EM path uses memory-mapped registers and separate polled RX and TX descriptor rings. Both Ethernet drivers share the same TCP and TLS code.

## Ethernet expansion and native Wi-Fi port

KreativeOS's Rust Intel driver uses PCI IDs cross-checked against [iPXE's Intel driver](https://github.com/ipxe/ipxe/blob/cb7718c14ae680f084398aa94b34ed285d5b011f/src/drivers/net/intel.c) and initializes the adapters through their shared legacy descriptor interface. It uses the firmware-loaded station address when valid and falls back to the controller-specific EEPROM read format.

FreeBSD's [iwlwifi build definition](https://github.com/freebsd/freebsd-src/blob/releng/15.0/sys/modules/iwlwifi/Makefile) includes AX210 configuration, firmware and PNVM loading, PCIe transport, and MVM. FreeBSD connects the driver to its wireless stack through LinuxKPI and linuxkpi_wlan. A native port needs PCIe DMA and interrupt handling, deferred work and timers, firmware loading, and the wireless association and key-management interface. The first hardware milestone is AX210 firmware startup and scan results, followed by WPA2 association and packet I/O. Current Wi-Fi still uses ESP-AT.

### AX210 firmware staging

Set `AX210_FIRMWARE=/path/to/iwlwifi-ty-a0-gf-a0-*.ucode` (use one exact filename) when running `bash scripts/build.sh`, or copy a firmware file to a firmware-readable FAT volume as `AX210.UCO` before boot. UEFI reads it into the kernel's bounded staging area. The kernel validates the Intel TLV container, reports the runtime and init section counts, and independently checks PCI for Intel device `8086:2725`. No Wi-Fi DMA starts at this point. This is a prerequisite for the AX210 PCIe transport, not an operational station driver.

When TY/GF firmware is staged and PCI exposes an AX210, the UEFI PCI I/O protocol reads the hardware revision and radio identity from BAR0. For the matching radio, the startup probe requests the PCI ownership bit, waits for acknowledgement, and restores the bit before returning. The Network window reports the stage reached. The probe has no device DMA or firmware submission. Ownership acknowledgement, timeout, and register restoration were checked against a simulated CSR interface. QEMU verified normal boot, but has no AX210 radio model.
