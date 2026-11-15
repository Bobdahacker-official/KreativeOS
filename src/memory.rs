use linked_list_allocator::LockedHeap;
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();
#[repr(align(4096))]
struct Arena([u8; 24 * 1024 * 1024]);
static mut ARENA: Arena = Arena([0; 24 * 1024 * 1024]);
pub unsafe fn init() {
    ALLOCATOR.lock().init(ARENA.0.as_mut_ptr(), ARENA.0.len());
}
