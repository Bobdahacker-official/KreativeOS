"""Build the dedicated, host-readable exFAT volume used by KreativeOS."""
import argparse
from pathlib import Path
import struct

SECTOR = 512
LENGTH = 65536
FAT = 128
FAT_SECTORS = 64
HEAP = 256
SPC = 8
CLUSTERS = (LENGTH - HEAP) // SPC
ROOT = 37
ROOT_CLUSTERS = 4
MARKER = b"KreativeOS dedicated exFAT\0\x01"


def checksum(data, skip=()):
    result = 0
    for i, byte in enumerate(data):
        if i not in skip:
            result = (((result >> 1) | (result << 31)) + byte) & 0xFFFFFFFF
    return result


def create(path, boot_image=None):
    path = Path(path)
    if path.suffix.lower() != '.img' or not path.parent.is_dir() or str(path).startswith(('\\\\.\\', '\\\\?\\')):
        raise ValueError("Choose a new .img file in an existing directory")
    boot = None
    if boot_image:
        source = Path(boot_image)
        if (source.suffix.lower() != '.img' or source.is_symlink() or not source.is_file()
                or source.stat().st_size != 64 * 1024 * 1024):
            raise ValueError("Choose the regular 64 MiB KreativeOS boot image file")
        boot = source.read_bytes()
    start = len(boot) // SECTOR if boot else 2048
    if boot and (len(boot) % (SECTOR * 2048) or start != 131072 or boot[510:512] != b'\x55\xaa'):
        raise ValueError("Expected the 64 MiB KreativeOS boot image")
    volume = bytearray(12 * SECTOR)
    volume[:11] = b'\xeb\x76\x90EXFAT   '
    struct.pack_into('<QQIIIII', volume, 64, start, LENGTH, FAT, FAT_SECTORS,
                     HEAP, CLUSTERS, ROOT)
    struct.pack_into('<IHHBBBBB', volume, 100, 0x4b4f5331, 0x0100, 0, 9, 3, 1, 0x80, 0xff)
    volume[510:512] = b'\x55\xaa'
    for i in range(1, 9):
        volume[(i + 1) * SECTOR - 4:(i + 1) * SECTOR] = b'\x00\x00\x55\xaa'
    check = checksum(volume[:11 * SECTOR], (106, 107, 112))
    volume[11 * SECTOR:] = struct.pack('<I', check) * 128

    # All 65,536 mappings are present. ASCII mappings are the required ones.
    up = bytearray()
    for value in range(65536):
        upper = chr(value).upper()
        translated = ord(upper) if len(upper) == 1 and ord(upper) <= 65535 else value
        up.extend(struct.pack('<H', translated))
    fat = bytearray(FAT_SECTORS * SECTOR)
    struct.pack_into('<II', fat, 0, 0xfffffff8, 0xffffffff)
    for start_cluster, count in ((2, 1), (3, 32), (ROOT, ROOT_CLUSTERS)):
        for cluster in range(start_cluster, start_cluster + count):
            struct.pack_into('<I', fat, cluster * 4, cluster + 1 if cluster + 1 < start_cluster + count else 0xffffffff)
    bitmap = bytearray((CLUSTERS + 7) // 8)
    for cluster in [2, *range(3, 35), *range(ROOT, ROOT + ROOT_CLUSTERS)]:
        index = cluster - 2
        bitmap[index // 8] |= 1 << (index % 8)
    root = bytearray(ROOT_CLUSTERS * SPC * SECTOR)
    root[0:2] = b'\x83\x06'
    root[2:14] = 'KOSDAT'.encode('utf-16le')
    root[32] = 0x81
    struct.pack_into('<IQ', root, 32 + 20, 2, len(bitmap))
    root[64] = 0x82
    struct.pack_into('<I', root, 64 + 4, checksum(up))
    struct.pack_into('<IQ', root, 64 + 20, 3, len(up))
    mbr = bytearray(boot[:SECTOR] if boot else bytes(SECTOR))
    entry = 462 if boot else 446
    struct.pack_into('<B3sB3sII', mbr, entry, 0, b'\xfe\xff\xff', 0x07,
                     b'\xfe\xff\xff', start, LENGTH)
    mbr[510:512] = b'\x55\xaa'

    with path.open('xb') as handle:
        if boot:
            handle.write(boot)
        handle.truncate((start + LENGTH) * SECTOR)
        def write(lba, data):
            handle.seek(lba * SECTOR)
            handle.write(data)
        def cluster_lba(cluster):
            return start + HEAP + (cluster - 2) * SPC
        write(0, mbr)
        write(16, MARKER)
        write(start, volume + volume)
        write(start + FAT, fat)
        write(cluster_lba(2), bitmap)
        write(cluster_lba(3), up)
        write(cluster_lba(ROOT), root)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--boot-image', type=Path)
    args = parser.parse_args()
    try:
        create(args.output, args.boot_image)
    except (OSError, ValueError) as error:
        parser.exit(1, f'{error}\n')
