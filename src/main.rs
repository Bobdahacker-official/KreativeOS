#![no_std]
#![no_main]
#![allow(static_mut_refs)]
mod boot;
mod graphics;
mod hardware;
mod storage;
mod net;
mod desktop;
mod text;

#[no_mangle]
pub unsafe extern "efiapi" fn efi_main(image: usize, system: *mut boot::SystemTable) -> usize {
    hardware::serial_init();
    hardware::log(b"KreativeOS: UEFI entry\r\n");
    if let Some(screen) = boot::prepare(image, system) {
        hardware::log(b"KreativeOS: boot services exited\r\n");
        hardware::init();
        storage::init();
        let network = net::Network::new();
        desktop::run(screen, network);
    }
    1
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { hardware::log(b"KreativeOS: kernel panic\r\n"); }
    loop { unsafe { core::arch::asm!("cli; hlt"); } }
}
