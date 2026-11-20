#![no_std]
#![no_main]
#![allow(static_mut_refs)]
extern crate alloc;
mod ahci;
mod archive;
mod audio;
mod ax210;
mod ax_dma;
mod boot;
mod desktop;
mod developer;
mod devices;
mod dhcp;
mod dns;
mod e1000;
mod ethernet;
mod exfat;
mod graphics;
mod hardware;
mod hardware_center;
mod memory;
mod net;
mod nvme;
mod organizer;
mod packages;
mod pictures;
mod ring3;
mod secure;
mod storage;
mod text;
mod usb;
mod users;
mod web;
mod wifi;

#[no_mangle]
pub unsafe extern "efiapi" fn efi_main(image: usize, system: *mut boot::SystemTable) -> usize {
    hardware::serial_init();
    hardware::log(b"KreativeOS: UEFI entry\r\n");
    if let Some(screen) = boot::prepare(image, system) {
        hardware::log(b"KreativeOS: boot services exited\r\n");
        memory::init();
        graphics::init();
        hardware::init();
        usb::init();
        storage::init();
        audio::init();
        let mut network = net::Network::new();
        #[cfg(not(feature = "tls-smoke"))]
        ethernet::configure_at_boot(&mut network);
        #[cfg(feature = "tls-smoke")]
        let network = {
            let mut network = network;
            secure::smoke(&mut network);
            network
        };
        desktop::run(screen, network);
    }
    1
}
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use core::fmt::Write;
    let mut diagnostic = text::Text::<320>::new();
    diagnostic.append("KreativeOS: kernel panic");
    if let Some(location) = info.location() {
        let _ = write!(diagnostic, " at {}:{}", location.file(), location.line());
    }
    let _ = write!(diagnostic, ": {}\r\n", info.message());
    unsafe {
        hardware::log(diagnostic.str().as_bytes());
    }
    loop {
        unsafe {
            core::arch::asm!("cli; hlt");
        }
    }
}
