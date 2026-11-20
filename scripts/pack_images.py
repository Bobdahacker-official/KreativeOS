#!/usr/bin/env python3
"""Pack local pictures for the KreativeOS gallery."""
from argparse import ArgumentParser
from pathlib import Path
import struct


def pack(paths):
    if len(paths) > 36:
        raise ValueError("Import at most 36 pictures per pack")
    output = bytearray(b"KIP1")
    for path in paths:
        name = path.name.encode("ascii")
        if not 1 <= len(name) <= 100 or any(c < 32 or c >= 127 for c in name):
            raise ValueError(f"Use a printable ASCII filename of at most 100 characters: {path}")
        size = path.stat().st_size
        if len(output) + len(name) + size + 5 > 6 * 1024 * 1024:
            raise ValueError("Image pack exceeds 6 MiB")
        data = path.read_bytes()
        if len(data) != size or not data.startswith((b"\x89PNG\r\n\x1a\n", b"\xff\xd8", b"BM")):
            raise ValueError(f"Expected a PNG, JPEG or BMP: {path}")
        output.extend(struct.pack("<BI", len(name), size))
        output.extend(name)
        output.extend(data)
    return output


if __name__ == "__main__":
    parser = ArgumentParser(description=__doc__)
    parser.add_argument("images", type=Path, nargs="+")
    parser.add_argument("-o", "--output", type=Path, default=Path("IMAGES.KIP"))
    args = parser.parse_args()
    try:
        args.output.write_bytes(pack(args.images))
    except (OSError, ValueError) as error:
        parser.error(str(error))
