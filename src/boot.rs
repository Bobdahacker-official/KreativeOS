//! UEFI ABI boundary. No firmware services are used after prepare returns.
use crate::graphics::Screen;
use core::ffi::c_void;
#[repr(C)]
pub struct Header {
    signature: u64,
    revision: u32,
    size: u32,
    crc: u32,
    reserved: u32,
}
#[repr(C)]
pub struct SystemTable {
    header: Header,
    vendor: usize,
    revision: u32,
    input_handle: usize,
    input: usize,
    output_handle: usize,
    output: usize,
    error_handle: usize,
    error: usize,
    runtime: usize,
    boot: *mut BootServices,
    entries: usize,
    tables: usize,
}
#[repr(C)]
struct BootServices {
    header: Header,
    raise: usize,
    restore: usize,
    allocate_pages: unsafe extern "efiapi" fn(u32, u32, usize, *mut u64) -> usize,
    free_pages: usize,
    memory_map:
        unsafe extern "efiapi" fn(*mut usize, *mut u8, *mut usize, *mut usize, *mut u32) -> usize,
    allocate_pool: usize,
    free_pool: usize,
    create_event: usize,
    set_timer: usize,
    wait_event: usize,
    signal_event: usize,
    close_event: usize,
    check_event: usize,
    install: usize,
    reinstall: usize,
    uninstall: usize,
    handle: unsafe extern "efiapi" fn(usize, *const Guid, *mut *mut c_void) -> usize,
    reserved: usize,
    register: usize,
    locate_handle: usize,
    locate_path: usize,
    install_config: usize,
    load: usize,
    start: usize,
    exit: usize,
    unload: usize,
    exit_boot: unsafe extern "efiapi" fn(usize, usize) -> usize,
    monotonic: usize,
    stall: unsafe extern "efiapi" fn(usize) -> usize,
    watchdog: unsafe extern "efiapi" fn(usize, u64, usize, *const u16) -> usize,
    connect: unsafe extern "efiapi" fn(usize, *mut usize, *mut c_void, u8) -> usize,
    disconnect: usize,
    open: usize,
    close: usize,
    open_info: usize,
    protocols: usize,
    locate_buffer: unsafe extern "efiapi" fn(
        u32,
        *const Guid,
        *mut c_void,
        *mut usize,
        *mut *mut usize,
    ) -> usize,
    locate: unsafe extern "efiapi" fn(*const Guid, *mut c_void, *mut *mut c_void) -> usize,
}
#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);
#[repr(C)]
struct Gop {
    query: usize,
    set: unsafe extern "efiapi" fn(*mut Gop, u32) -> usize,
    blt: usize,
    mode: *mut Mode,
}
#[repr(C)]
struct Mode {
    max: u32,
    current: u32,
    info: *mut Info,
    size: usize,
    base: u64,
    bytes: usize,
}
#[repr(C)]
struct Info {
    version: u32,
    width: u32,
    height: u32,
    format: u32,
    masks: [u32; 4],
    stride: u32,
}
static mut MAP: [u8; 131072] = [0; 131072];
pub unsafe fn prepare(image: usize, st: *mut SystemTable) -> Option<Screen> {
    let bs = &mut *(*st).boot;
    (bs.watchdog)(0, 0, 0, core::ptr::null());
    let guid = Guid(
        0x9042a9de,
        0x23dc,
        0x4a38,
        [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
    );
    let mut ptr = core::ptr::null_mut();
    if (bs.locate)(&guid, core::ptr::null_mut(), &mut ptr) != 0 {
        return None;
    }
    let gop = ptr as *mut Gop;
    // Select a supported 1024x768 or 1280x800 mode, freeing query allocations.
    type Query = unsafe extern "efiapi" fn(*mut Gop, u32, *mut usize, *mut *mut Info) -> usize;
    type Free = unsafe extern "efiapi" fn(*mut c_void) -> usize;
    let query: Query = core::mem::transmute((*gop).query);
    let free: Free = core::mem::transmute(bs.free_pool);
    for n in 0..(*(*gop).mode).max {
        let mut size = 0;
        let mut info = core::ptr::null_mut();
        if query(gop, n, &mut size, &mut info) == 0 {
            let chosen = (*info).width == 1024 && (*info).height == 768 && (*info).format <= 1;
            free(info.cast());
            if chosen {
                ((*gop).set)(gop, n);
                break;
            }
        }
    }
    let m = &*(*gop).mode;
    let i = &*m.info;
    if i.format > 1 || i.width < 640 || i.height < 480 || i.width > 1920 || i.height > 1200 {
        return None;
    }
    let mut back = 0u64;
    let pixels = i.width as usize * i.height as usize;
    if (bs.allocate_pages)(0, 2, (pixels * 4 + 4095) / 4096, &mut back) != 0 {
        return None;
    }
    // DMA buffers are explicitly allocated below 4GiB for the RTL8139 controller.
    let mut dma = 0xffff_ffffu64;
    if (bs.allocate_pages)(1, 2, 40, &mut dma) != 0 {
        return None;
    }
    crate::net::DMA = dma as usize;
    let mut audio_dma = 0xffff_ffffu64;
    if (bs.allocate_pages)(1, 2, crate::audio::DMA_SIZE / 4096, &mut audio_dma) == 0 {
        crate::audio::DMA = audio_dma as usize;
    }
    connect_usb_storage(bs, free);
    load_pictures(bs, free);
    prepare_ax210_dma(bs);
    inspect_ax210(bs, free);
    let screen = Screen {
        front: m.base as *mut u32,
        back: back as *mut u32,
        w: i.width as usize,
        h: i.height as usize,
        stride: i.stride as usize,
        rgb: i.format == 0,
        clip: None,
        theme: 0,
    };
    for _ in 0..4 {
        let mut size = MAP.len();
        let mut key = 0;
        let mut desc = 0;
        let mut version = 0;
        if (bs.memory_map)(
            &mut size,
            MAP.as_mut_ptr(),
            &mut key,
            &mut desc,
            &mut version,
        ) != 0
        {
            return None;
        }
        if (bs.exit_boot)(image, key) == 0 {
            core::arch::asm!("cli");
            return Some(screen);
        }
    }
    None
}

#[repr(C)]
struct SimpleFileSystem {
    revision: u64,
    open_volume: unsafe extern "efiapi" fn(*mut Self, *mut *mut EfiFile) -> usize,
}

#[repr(C)]
struct EfiFile {
    revision: u64,
    open: unsafe extern "efiapi" fn(*mut Self, *mut *mut Self, *const u16, u64, u64) -> usize,
    close: unsafe extern "efiapi" fn(*mut Self) -> usize,
    delete: usize,
    read: unsafe extern "efiapi" fn(*mut Self, *mut usize, *mut u8) -> usize,
}

type BarAccess = unsafe extern "efiapi" fn(*mut PciIo, u32, u8, u64, usize, *mut u32) -> usize;

#[repr(C)]
struct PciIo {
    poll: [usize; 2],
    mem_read: BarAccess,
    mem_write: BarAccess,
    io: [usize; 2],
    config_read: unsafe extern "efiapi" fn(*mut Self, u32, u32, usize, *mut u32) -> usize,
}

unsafe fn prepare_ax210_dma(bs: &BootServices) {
    use crate::{ax210, ax_dma};
    if ax210::staged_firmware().is_none() {
        return;
    }
    let mut address = 0xffff_ffffu64;
    let pages = ax_dma::ARENA_SIZE / 4096;
    if (bs.allocate_pages)(1, 2, pages, &mut address) != 0 {
        return;
    }
    let arena = core::slice::from_raw_parts_mut(address as *mut u8, ax_dma::ARENA_SIZE);
    let bytes = &ax210::FIRMWARE[..ax210::FIRMWARE_LEN];
    ax_dma::BOOTSTRAP = ax_dma::prepare(bytes, arena, address);
    if ax_dma::BOOTSTRAP.is_none() {
        let free: unsafe extern "efiapi" fn(u64, usize) -> usize =
            core::mem::transmute(bs.free_pages);
        free(address, pages);
    }
}

unsafe fn inspect_ax210(bs: &BootServices, free: unsafe extern "efiapi" fn(*mut c_void) -> usize) {
    use crate::ax210::{self, HardwareState};
    let guid = Guid(
        0x4cf5b200,
        0x68b8,
        0x4ca5,
        [0x9e, 0xec, 0xb2, 0x3e, 0x3f, 0x50, 0x02, 0x9a],
    );
    let mut count = 0;
    let mut handles = core::ptr::null_mut();
    if (bs.locate_buffer)(2, &guid, core::ptr::null_mut(), &mut count, &mut handles) != 0 {
        return;
    }
    for index in 0..count.min(512) {
        let mut protocol = core::ptr::null_mut();
        let handle = *handles.add(index);
        if (bs.handle)(handle, &guid, &mut protocol) != 0 {
            continue;
        }
        let pci = protocol.cast::<PciIo>();
        let mut id = 0;
        if ((*pci).config_read)(pci, 2, 0, 1, &mut id) != 0 || id != 0x2725_8086 {
            continue;
        }
        let read = |offset| {
            let mut value = 0;
            (((*pci).mem_read)(pci, 2, 0, offset, 1, &mut value) == 0 && value != u32::MAX)
                .then_some(value)
        };
        let write = |offset, mut value| ((*pci).mem_write)(pci, 2, 0, offset, 1, &mut value) == 0;
        // Never touch device registers unless a complete firmware container was staged.
        ax210::HARDWARE = if let Some(dma) = crate::ax_dma::BOOTSTRAP.as_ref() {
            ax210::prepare_bar(
                read,
                write,
                || {
                    (bs.stall)(10);
                },
                |revision| dma.set_mac_revision(revision),
            )
        } else {
            HardwareState::NotAttempted
        };
        break;
    }
    free(handles.cast());
}

unsafe fn connect_usb_storage(
    bs: &BootServices,
    free: unsafe extern "efiapi" fn(*mut c_void) -> usize,
) {
    let pci_io_guid = Guid(
        0x4cf5b200,
        0x68b8,
        0x4ca5,
        [0x9e, 0xec, 0xb2, 0x3e, 0x3f, 0x50, 0x02, 0x9a],
    );
    let mut count = 0;
    let mut handles = core::ptr::null_mut();
    if (bs.locate_buffer)(
        2,
        &pci_io_guid,
        core::ptr::null_mut(),
        &mut count,
        &mut handles,
    ) != 0
    {
        return;
    }
    for index in 0..count.min(512) {
        let handle = *handles.add(index);
        let mut protocol = core::ptr::null_mut();
        if (bs.handle)(handle, &pci_io_guid, &mut protocol) != 0 {
            continue;
        }
        let pci = protocol.cast::<PciIo>();
        let mut class = 0u32;
        if ((*pci).config_read)(pci, 2, 8, 1, &mut class) == 0 && class >> 16 == 0x0c03 {
            (bs.connect)(handle, core::ptr::null_mut(), core::ptr::null_mut(), 1);
        }
    }
    free(handles.cast());
}

unsafe fn load_pictures(bs: &BootServices, free: unsafe extern "efiapi" fn(*mut c_void) -> usize) {
    use crate::pictures::{BOOT_IMAGES, IMPORT_LIMIT, VOLUMES};
    let guid = Guid(
        0x964e5b22,
        0x6459,
        0x11d2,
        [0x8e, 0x39, 0, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
    );
    let filename = [92u16, 73, 77, 65, 71, 69, 83, 46, 75, 73, 80, 0];
    let mut count = 0;
    let mut handles = core::ptr::null_mut();
    if (bs.locate_buffer)(2, &guid, core::ptr::null_mut(), &mut count, &mut handles) != 0 {
        return;
    }
    let mut offset = 0;
    let mut imported = 0;
    for i in 0..count.min(128) {
        let mut protocol = core::ptr::null_mut();
        if (bs.handle)(*handles.add(i), &guid, &mut protocol) != 0 {
            continue;
        }
        let fs = protocol.cast::<SimpleFileSystem>();
        let mut root = core::ptr::null_mut();
        if ((*fs).open_volume)(fs, &mut root) != 0 {
            continue;
        }
        use crate::ax210::{FIRMWARE, FIRMWARE_LEN, MAX_FIRMWARE};
        if FIRMWARE_LEN == 0 {
            // FAT 8.3 name: AX210.UCO.
            let firmware_name = [92u16, 65, 88, 50, 49, 48, 46, 85, 67, 79, 0];
            let mut firmware = core::ptr::null_mut();
            if ((*root).open)(root, &mut firmware, firmware_name.as_ptr(), 1, 0) == 0 {
                let mut length = MAX_FIRMWARE;
                let result = ((*firmware).read)(firmware, &mut length, FIRMWARE.as_mut_ptr());
                let mut count = 1;
                let mut byte = 0;
                let eof = ((*firmware).read)(firmware, &mut count, &mut byte) == 0 && count == 0;
                ((*firmware).close)(firmware);
                if result == 0 && eof && length <= MAX_FIRMWARE {
                    FIRMWARE_LEN = length;
                }
            }
        }
        let mut file = core::ptr::null_mut();
        let status = ((*root).open)(root, &mut file, filename.as_ptr(), 1, 0);
        if status == 0 {
            let mut length = IMPORT_LIMIT - offset;
            let status = ((*file).read)(file, &mut length, BOOT_IMAGES.as_mut_ptr().add(offset));
            let mut extra = 1;
            let mut tail = 0;
            let end = ((*file).read)(file, &mut extra, &mut tail);
            ((*file).close)(file);
            if imported < VOLUMES.len()
                && status == 0
                && end == 0
                && extra == 0
                && length >= 4
                && length <= IMPORT_LIMIT - offset
                && &BOOT_IMAGES[offset..offset + 4] == b"KIP1"
            {
                VOLUMES[imported] = (offset, length);
                offset += length;
                imported += 1;
            }
        }
        ((*root).close)(root);
        if (imported == VOLUMES.len() || offset >= IMPORT_LIMIT - 4) && FIRMWARE_LEN != 0 {
            break;
        }
    }
    free(handles.cast());
}
