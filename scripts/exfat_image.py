"""Create a new exFAT data image with a KOSDAT volume label."""
import argparse
from pathlib import Path
import struct
import uuid
import zlib

VOLUME_BYTES = 32 * 1024 * 1024
BASIC_DATA = uuid.UUID('ebd0a0a2-b9e5-4433-87c0-68b6b72699c7')


def checksum(data, skip=()):
    result = 0
    for index, byte in enumerate(data):
        if index not in skip:
            result = (((result >> 1) | (result << 31)) + byte) & 0xFFFFFFFF
    return result


def mbr_entry(mbr, index, kind, first, count):
    struct.pack_into('<B3sB3sII', mbr, 446 + index * 16, 0, b'\xfe\xff\xff',
                     kind, b'\xfe\xff\xff', first, count)


def gpt_tables(sector, first, count):
    entries = bytearray(128 * 128)
    struct.pack_into('<16s16sQQQ', entries, 0, BASIC_DATA.bytes_le,
                     uuid.uuid4().bytes_le, first, first + count - 1, 0)
    label = 'KreativeOS Data'.encode('utf-16le')
    entries[56:56 + len(label)] = label
    array_blocks = len(entries) // sector
    backup_array = first + count
    last = backup_array + array_blocks
    disk_id = uuid.uuid4().bytes_le

    def header(lba, peer, array):
        result = bytearray(sector)
        struct.pack_into('<8sIIIIQQQQ16sQIII', result, 0, b'EFI PART',
                         0x10000, 92, 0, 0, lba, peer, 2 + array_blocks,
                         backup_array - 1, disk_id, array, 128, 128, zlib.crc32(entries))
        struct.pack_into('<I', result, 16, zlib.crc32(result[:92]))
        return result

    return last, ((1, header(1, last, 2)), (2, entries),
                  (backup_array, entries), (last, header(last, 1, backup_array)))


def create(path, boot_image=None, *, partition='mbr', sector_size=512, cluster_size=4096):
    path = Path(path)
    if (path.suffix.lower() != '.img' or not path.parent.is_dir()
            or str(path).startswith(('\\\\.\\', '\\\\?\\'))):
        raise ValueError("Choose a new .img file in an existing directory")
    if sector_size not in (512, 4096) or cluster_size not in (4096, 8192, 16384, 32768):
        raise ValueError("Unsupported sector or cluster size")
    if partition not in ('mbr', 'gpt', 'none'):
        raise ValueError("Unsupported partition table")
    boot = None
    if boot_image:
        if partition != 'mbr' or sector_size != 512:
            raise ValueError("The combined EFI boot image requires 512-byte sectors and MBR")
        source = Path(boot_image)
        if (source.suffix.lower() != '.img' or source.is_symlink() or not source.is_file()
                or source.stat().st_size != 64 * 1024 * 1024):
            raise ValueError("Choose the regular 64 MiB KreativeOS boot image file")
        boot = source.read_bytes()
        if boot[510:512] != b'\x55\xaa':
            raise ValueError("Expected the 64 MiB KreativeOS boot image")

    sector = sector_size
    volume_sectors = VOLUME_BYTES // sector
    sectors_per_cluster = cluster_size // sector
    start = (len(boot) // sector if boot else
             0 if partition == 'none' else (1024 * 1024) // sector)
    fat_offset = 24
    fat_sectors = ((volume_sectors // sectors_per_cluster + 2) * 4 + sector - 1) // sector
    heap = ((fat_offset + fat_sectors + sectors_per_cluster - 1)
            // sectors_per_cluster * sectors_per_cluster)
    clusters = (volume_sectors - heap) // sectors_per_cluster
    bitmap = bytearray((clusters + 7) // 8)
    bitmap_clusters = (len(bitmap) + cluster_size - 1) // cluster_size
    upcase_start = 2 + bitmap_clusters
    upcase = bytearray()
    for value in range(65536):
        upper = chr(value).upper()
        translated = ord(upper) if len(upper) == 1 and ord(upper) <= 65535 else value
        upcase.extend(struct.pack('<H', translated))
    upcase_clusters = (len(upcase) + cluster_size - 1) // cluster_size
    root_start = upcase_start + upcase_clusters
    root_clusters = 1

    boot_region = bytearray(12 * sector)
    boot_region[:11] = b'\xeb\x76\x90EXFAT   '
    struct.pack_into('<QQIIIII', boot_region, 64, start, volume_sectors, fat_offset,
                     fat_sectors, heap, clusters, root_start)
    used = bitmap_clusters + upcase_clusters + root_clusters
    struct.pack_into('<IHHBBBBB', boot_region, 100, uuid.uuid4().int & 0xffffffff,
                     0x0100, 0, sector.bit_length() - 1, sectors_per_cluster.bit_length() - 1,
                     1, 0x80, used * 100 // clusters)
    boot_region[510:512] = b'\x55\xaa'
    for index in range(1, 9):
        boot_region[(index + 1) * sector - 4:(index + 1) * sector] = b'\x00\x00\x55\xaa'
    check = checksum(boot_region[:11 * sector], (106, 107, 112))
    boot_region[11 * sector:] = struct.pack('<I', check) * (sector // 4)

    fat = bytearray(fat_sectors * sector)
    struct.pack_into('<II', fat, 0, 0xfffffff8, 0xffffffff)
    for first, count in ((2, bitmap_clusters), (upcase_start, upcase_clusters),
                         (root_start, root_clusters)):
        for cluster in range(first, first + count):
            struct.pack_into('<I', fat, cluster * 4,
                             cluster + 1 if cluster + 1 < first + count else 0xffffffff)
            index = cluster - 2
            bitmap[index // 8] |= 1 << (index % 8)
    root = bytearray(root_clusters * cluster_size)
    root[0:2] = b'\x83\x06'
    root[2:14] = 'KOSDAT'.encode('utf-16le')
    root[32] = 0x81
    struct.pack_into('<IQ', root, 32 + 20, 2, len(bitmap))
    root[64] = 0x82
    struct.pack_into('<I', root, 64 + 4, checksum(upcase))
    struct.pack_into('<IQ', root, 64 + 20, upcase_start, len(upcase))

    last = start + volume_sectors - 1
    tables = ()
    mbr = bytearray(boot[:sector] if boot else bytes(sector))
    if partition == 'gpt':
        last, tables = gpt_tables(sector, start, volume_sectors)
        mbr_entry(mbr, 0, 0xee, 1, last)
    elif partition == 'mbr':
        mbr_entry(mbr, 1 if boot else 0, 7, start, volume_sectors)
    mbr[510:512] = b'\x55\xaa'

    with path.open('xb') as handle:
        if boot:
            handle.write(boot)
        handle.truncate((last + 1) * sector)

        def write(lba, data):
            handle.seek(lba * sector)
            handle.write(data)

        def cluster_lba(cluster):
            return start + heap + (cluster - 2) * sectors_per_cluster

        if partition != 'none':
            write(0, mbr)
        for lba, table in tables:
            write(lba, table)
        write(start, boot_region + boot_region)
        write(start + fat_offset, fat)
        write(cluster_lba(2), bitmap)
        write(cluster_lba(upcase_start), upcase)
        write(cluster_lba(root_start), root)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--boot-image', type=Path)
    parser.add_argument('--partition', choices=('mbr', 'gpt', 'none'), default='mbr')
    parser.add_argument('--sector-size', type=int, choices=(512, 4096), default=512)
    parser.add_argument('--cluster-size', type=int, choices=(4096, 8192, 16384, 32768), default=4096)
    args = parser.parse_args()
    try:
        create(args.output, args.boot_image, partition=args.partition, sector_size=args.sector_size,
               cluster_size=args.cluster_size)
    except (OSError, ValueError) as error:
        parser.exit(1, f'{error}\n')
