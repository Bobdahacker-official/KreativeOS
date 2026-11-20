#!/usr/bin/env python3
"""Create a dedicated KFS SATA or NVMe image from scratch or from a legacy IDE image."""
import argparse
from pathlib import Path

SECTOR = 512
SIZE = 8 * 1024 * 1024
MARKER_OFFSET = 16 * SECTOR
MARKER = b"KreativeOS KFS AHCI data disk\0\x01"
NVME_MARKER = b"KreativeOS KFS data disk\0\x01"


def valid_snapshot(data):
    if data[:4] != b"KFS1":
        return False
    checksum = 2166136261
    for value in data[SECTOR:]:
        checksum = ((checksum ^ value) * 16777619) & 0xFFFFFFFF
    return checksum == int.from_bytes(data[8:12], "little")


def create(destination, source=None, marker=MARKER):
    original = None
    if source is not None:
        with source.open("rb") as image:
            original = image.read(SIZE + 1)
        if len(original) != SIZE or any(original[:2048 * SECTOR]):
            raise ValueError("Expected a dedicated 8 MiB IDE image without partitions or a SATA marker")
        snapshots = [original[lba * SECTOR:(lba + 100) * SECTOR] for lba in (2048, 2148)]
        tail = original[2258 * SECTOR:]
        if any(tail) or (any(original) and not any(map(valid_snapshot, snapshots))):
            raise ValueError("Source is neither blank nor a recognized KFS image")
    with destination.open("xb") as image:
        if original is None:
            image.truncate(SIZE)
        else:
            image.write(original)
        image.seek(MARKER_OFFSET)
        image.write(marker)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path, help="new data image file")
    parser.add_argument("--from-ide", type=Path, help="existing KFS IDE image (read only)")
    parser.add_argument("--nvme", action="store_true", help="mark the image for native NVMe or SATA")
    opts = parser.parse_args()
    try:
        create(opts.destination, opts.from_ide, NVME_MARKER if opts.nvme else MARKER)
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")
    print(f"Created {opts.destination}")
