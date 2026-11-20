#![no_std]
#![no_main]
#![allow(static_mut_refs)]
extern crate alloc;
mod audio;
mod boot;
mod desktop;
mod e1000;
mod graphics;
mod hardware;
mod memory;
mod net;
mod pictures;
mod secure;
mod storage;
mod text;
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
        storage::init();
        audio::init();
        let network = net::Network::new();
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
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe {
        hardware::log(b"KreativeOS: kernel panic\r\n");
    }
    loop {
        unsafe {
            core::arch::asm!("cli; hlt");
        }
    }
}
