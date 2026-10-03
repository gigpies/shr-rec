use shr_rec::recorder::{Config, Recorder};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct GuardAllocator;
thread_local! { static CALLBACK: Cell<bool> = const { Cell::new(false) }; static ALLOCATIONS: Cell<usize> = const { Cell::new(0) }; }
unsafe impl GlobalAlloc for GuardAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        CALLBACK.with(|active| {
            if active.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        CALLBACK.with(|active| {
            if active.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: GuardAllocator = GuardAllocator;

#[test]
fn production_push_and_fault_paths_do_not_allocate_or_free() {
    let path = std::env::temp_dir().join(format!("shr-rec-callback-{}", std::process::id()));
    let mut recorder = Recorder::create(
        &path,
        Config {
            sample_rate: 48_000,
            channels: 8,
            max_block: 192,
            queue_blocks: 2,
            epoch: 11,
        },
    )
    .unwrap();
    let samples = [0.0; 8 * 192];
    CALLBACK.with(|active| active.set(true));
    let first = recorder.push(0, &samples, 192);
    let invalid = recorder.push(192, &[f64::NAN], 192);
    for index in 1..100 {
        let _ = recorder.push(index * 192, &samples, 192);
    }
    recorder.mark_fault(7);
    CALLBACK.with(|active| active.set(false));
    assert_eq!(first, 0);
    assert_eq!(invalid, -1);
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert_eq!(recorder.finish().unwrap().status, "incomplete");
    std::fs::remove_dir_all(path).unwrap();
}
