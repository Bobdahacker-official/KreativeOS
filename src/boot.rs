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
    handle: usize,
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
    stall: usize,
    watchdog: unsafe extern "efiapi" fn(usize, u64, usize, *const u16) -> usize,
    connect: usize,
    disconnect: usize,
    open: usize,
    close: usize,
    open_info: usize,
    protocols: usize,
    locate_buffer: usize,
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
    if (bs.allocate_pages)(1, 2, 8, &mut dma) != 0 {
        return None;
    }
    crate::net::DMA = dma as usize;
    let screen = Screen {
        front: m.base as *mut u32,
        back: back as *mut u32,
        w: i.width as usize,
        h: i.height as usize,
        stride: i.stride as usize,
        rgb: i.format == 0,
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
