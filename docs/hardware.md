# Storage hardware and driver research

KreativeOS discovers native SATA controllers by PCI class, subclass and programming interface `01:06:01`. This covers standard AHCI implementations without tying probe results to a list of vendor IDs. The driver checks that BAR5 is an assigned 32-bit MMIO range reachable through the existing firmware page tables. If the host advertises BIOS/OS handoff, the driver requests ownership and waits for firmware to release it. See sections 3.1, 3.3 and 10.6 of the [Intel AHCI 1.3.1 specification](https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/serial-ata-ahci-spec-rev1-3-1.pdf).

The port engine stops before the driver changes the command list or received FIS addresses. Some controllers leave the device signature unset until the first register FIS arrives. In that case, the driver issues a port reset while command issue is stopped and FIS reception is enabled. It then waits for a usable link and an ATA disk signature. The driver uses the specification's aligned command and FIS structures, a single DMA slot, and bounded waits for port readiness, command completion and cache flush. Relevant details are in sections 4.2, 5.3, 10.3 and 10.4 of the [Intel AHCI specification](https://www.intel.com/content/dam/www/public/us/en/documents/technical-specifications/serial-ata-ahci-spec-rev1-3-1.pdf).

The driver checks DMA and 48-bit LBA support, logical sector size and disk capacity through ATA IDENTIFY DEVICE. It uses READ DMA EXT, WRITE DMA EXT and FLUSH CACHE EXT. It writes the inactive KFS snapshot in three ordered steps: invalidate the header, write and flush the body, then publish and flush the header. The [T13 ATA8-ACS working drafts](https://www.t13.org/project-working-drafts?page=6) describe the ATA commands and IDENTIFY fields. [QEMU's ATA implementation](https://github.com/qemu/qemu/blob/master/hw/ide/core.c) is a cross-check for test-controller behavior.

## What the driver accepts

| Supported path | Requirements |
|---|---|
| Physical SATA disk or SSD | Standard AHCI controller, directly attached SATA drive, 512-byte logical sectors, 48-bit LBA and a marked KFS image |
| QEMU SATA profile | ICH9 AHCI with a dedicated data image. The guest negotiates and executes its own DMA commands. [QEMU AHCI implementation](https://github.com/qemu/qemu/blob/master/hw/ide/ahci.c) |
| Physical PCIe NVMe SSD | Standard NVMe PCIe controller, a 512-byte namespace without metadata or protection information, and marked KFS media |
| QEMU NVMe profile | PCIe NVMe controller with a marked KFS namespace. [QEMU NVMe documentation](https://www.qemu.org/docs/master/system/devices/nvme.html) |
| Legacy IDE | Existing primary-master KFS data images remain usable with the IDE launch profile. |

The kernel polls command completion and gives each native controller driver a separate DMA arena below 4 GiB. There is no automatic repartitioning or disk hotplug workflow. AHCI does not perform a controller-wide reset. RAID, ATAPI and USB storage use separate transports outside these drivers' scope.

## NVMe PCIe

The kernel takes control of the PCIe controller through BAR0 and enables the NVM command set with 4 KiB DMA pages. It negotiates one I/O queue pair, issues Identify to enumerate active namespaces, and filters for a compatible block format before reading any KFS metadata. Submission and completion queues track phase, command ID and queue ID. Transfers use physical region page lists when the KFS record spans more than two DMA pages. Completion timeouts stop further submissions. The three KFS write stages each issue an NVMe Flush command. See [NVMe Base 2.0d](https://nvmexpress.org/wp-content/uploads/NVM-Express-Base-Specification-2.0d-2024.01.11-Ratified.pdf) and [NVM Command Set 1.0c](https://nvmexpress.org/wp-content/uploads/NVM-Express-NVM-Command-Set-Specification-1.0c-2022.10.03-Ratified.pdf).
