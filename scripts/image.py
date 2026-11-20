#!/usr/bin/env python3
"""Build a partitioned FAT16 UEFI boot image without third-party tooling."""
from pathlib import Path
import struct, os
from pack_images import pack
ROOT = Path(__file__).resolve().parent.parent
out = ROOT / os.environ.get('KREATIVE_BUILD_DIR', 'build')
loader = (out / 'esp/EFI/BOOT/BOOTX64.EFI').read_bytes()
sector = 512
start = 2048
count = 129024
spc = 8
fat_sectors = 64
reserved = 1
root_sectors = 32
clusters = (count - reserved - 2 * fat_sectors - root_sectors) // spc
assert 4085 < clusters < 65525
image = bytearray((start + count) * sector)
# MBR, one EFI System Partition.
image[446:462] = struct.pack('<B3sB3sII', 0, b'\x00\x02\x00', 0xEF,
                             b'\xfe\xff\xff', start, count)
image[510:512] = b'\x55\xaa'
base = start * sector
boot = bytearray(sector)
boot[:3] = b'\xeb\x3c\x90'
boot[3:11] = b'KREATIVE'
struct.pack_into('<HBHBHHBHHHII', boot, 11, sector, spc, reserved, 2,
                 512, 0, 0xF8, fat_sectors, 63, 255, start, count)
boot[36] = 0x80
boot[38] = 0x29
struct.pack_into('<I', boot, 39, 0x4B524F53)
boot[43:54] = b'KREATIVEOS '
boot[54:62] = b'FAT16   '
boot[510:512] = b'\x55\xaa'
image[base:base+512] = boot
fat = bytearray(fat_sectors * sector)
struct.pack_into('<HH', fat, 0, 0xfff8, 0xffff)
data_start = base + (reserved + 2 * fat_sectors + root_sectors) * sector
cluster_bytes = spc * sector
next_cluster = 2

def allocate(data):
    global next_cluster
    number = max(1, (len(data) + cluster_bytes - 1) // cluster_bytes)
    first = next_cluster
    assert first + number < clusters + 2
    for i in range(number):
        cl = next_cluster
        next_cluster += 1
        struct.pack_into('<H', fat, cl * 2, 0xffff if i == number-1 else cl+1)
        pos = data_start + (cl-2) * cluster_bytes
        chunk = data[i*cluster_bytes:(i+1)*cluster_bytes]
        image[pos:pos+len(chunk)] = chunk
    return first

def entry(name, attr, cluster, size=0):
    e = bytearray(32)
    e[:11] = name.encode('ascii').ljust(11, b' ')
    e[11] = attr
    struct.pack_into('<H', e, 26, cluster)
    struct.pack_into('<I', e, 28, size)
    return e

efi = allocate(b'')
bootdir = allocate(b'')
file_cluster = allocate(loader)
for cl, parent, entries in [(efi, 0, [entry('BOOT', 16, bootdir)]),
                           (bootdir, efi, [entry('BOOTX64 EFI', 32, file_cluster, len(loader))])]:
    directory = entry('.', 16, cl) + entry('..', 16, parent) + b''.join(entries)
    pos = data_start + (cl-2) * cluster_bytes
    image[pos:pos+len(directory)] = directory
root_pos = base + (reserved + 2 * fat_sectors) * sector
image[root_pos:root_pos+32] = entry('EFI', 16, efi)
imports = ROOT / os.environ.get('KREATIVE_IMPORT_DIR', 'imports')
if imports.is_dir():
    paths = [p for p in sorted(imports.iterdir()) if p.is_file() and p.suffix.lower() in {'.png', '.jpg', '.jpeg', '.bmp'}]
    if paths:
        pictures = pack(paths)
        picture_cluster = allocate(pictures)
        image[root_pos+32:root_pos+64] = entry('IMAGES  KIP', 32, picture_cluster, len(pictures))
for i in range(2):
    pos = base + (reserved + i * fat_sectors) * sector
    image[pos:pos+len(fat)] = fat
(out / 'KreativeOS.img').write_bytes(image)
# Dedicated, blank KFS disk. Never overwrite an existing user's disk.
if not (out / 'KreativeOS-data.img').exists():
    with (out / 'KreativeOS-data.img').open('wb') as f:
        f.truncate(8 * 1024 * 1024)
print(f'Built KreativeOS.img: {len(image)} bytes; EFI loader: {len(loader)} bytes')
