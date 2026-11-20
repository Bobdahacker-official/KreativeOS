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
