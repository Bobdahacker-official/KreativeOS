#!/usr/bin/env python3
"""Pack a directory into a Linux newc initramfs with deterministic metadata."""
import gzip
import os
from pathlib import Path
import stat
import sys

root = Path(sys.argv[1])

def pad(stream, n):
    stream.write(b"\0" * (-n % 4))

def add(stream, ino, name, mode, data):
    fields = (ino, mode, 0, 0, 1, 0, len(data), 0, 0, 0, 0, len(name) + 1, 0)
    header = ("070701" + "".join(f"{value:08x}" for value in fields)).encode()
    stream.write(header + name + b"\0")
    pad(stream, len(header) + len(name) + 1)
    stream.write(data)
    pad(stream, len(data))

with open(sys.argv[2], "wb") as target, gzip.GzipFile(fileobj=target, filename="", mode="wb", mtime=0) as archive:
    last = 0
    for last, path in enumerate(sorted(root.rglob("*")), 1):
        mode = path.lstat().st_mode
        if stat.S_ISREG(mode):
            data = path.read_bytes()
        elif stat.S_ISLNK(mode):
            data = os.fsencode(os.readlink(path))
        elif stat.S_ISDIR(mode):
            data = b""
        else:
            raise ValueError(f"Unsupported file type for {path}")
        add(archive, last, os.fsencode(path.relative_to(root)), mode, data)
    add(archive, last + 1, b"TRAILER!!!", 0, b"")
