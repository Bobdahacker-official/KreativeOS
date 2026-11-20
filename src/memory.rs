use linked_list_allocator::LockedHeap;
#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();
#[repr(align(4096))]
struct Arena([u8; 48 * 1024 * 1024]);
static mut ARENA: Arena = Arena([0; 48 * 1024 * 1024]);
pub unsafe fn init() {
    ALLOCATOR.lock().init(ARENA.0.as_mut_ptr(), ARENA.0.len());
}
pub fn usage() -> (usize, usize) {
    let heap = ALLOCATOR.lock();
    (heap.used(), heap.size())
}
