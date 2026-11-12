mod support;
use shr_rec::{
    playback::{self, State},
    take::Take,
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
use support::*;
struct Guard;
thread_local! { static ACTIVE:Cell<bool>=const{Cell::new(false)};static OPS:Cell<usize>=const{Cell::new(0)};}
unsafe impl GlobalAlloc for Guard {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ACTIVE.with(|a| {
            if a.get() {
                OPS.with(|n| n.set(n.get() + 1))
            }
        });
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        ACTIVE.with(|a| {
            if a.get() {
                OPS.with(|n| n.set(n.get() + 1))
            }
        });
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOCATOR: Guard = Guard;
#[test]
fn callback_play_pause_eof_underrun_and_oversize_never_allocate_or_free() {
    let root = Scratch::new();
    let path = shr(&root.0, 100000, 2);
    let (mut r, w) =
        playback::prepare(Take::open(&path).unwrap(), &exact(2), 48000, false, 0).unwrap();
    drop(w);
    let mut a = [0.; 8192];
    let mut b = [0.; 8192];
    r.shared.play().unwrap();
    ACTIVE.with(|a| a.set(true));
    r.process(&mut [&mut a, &mut b]);
    r.shared.pause();
    r.process(&mut [&mut a, &mut b]);
    r.shared.play().unwrap();
    for _ in 0..6 {
        r.process(&mut [&mut a, &mut b]);
    }
    ACTIVE.with(|a| a.set(false));
    assert_eq!(r.shared.state(), State::Underrun);
    assert_eq!(OPS.with(Cell::get), 0);
    let (mut r, _w) =
        playback::prepare(Take::open(&path).unwrap(), &exact(2), 48000, false, 99999).unwrap();
    r.shared.play().unwrap();
    ACTIVE.with(|a| a.set(true));
    r.process(&mut [&mut a, &mut b]);
    ACTIVE.with(|a| a.set(false));
    assert_eq!(r.shared.state(), State::Eof);
    assert_eq!(OPS.with(Cell::get), 0);
    let (mut r, _w) =
        playback::prepare(Take::open(&path).unwrap(), &exact(2), 48000, false, 0).unwrap();
    r.shared.play().unwrap();
    let mut huge_a = [99.; 8193];
    let mut huge_b = [99.; 8193];
    ACTIVE.with(|a| a.set(true));
    r.process(&mut [&mut huge_a, &mut huge_b]);
    ACTIVE.with(|a| a.set(false));
    assert_eq!(r.shared.state(), State::InvalidPeriod);
    assert!(huge_a.iter().chain(&huge_b).all(|v| *v == 0.));
    assert_eq!(OPS.with(Cell::get), 0);
}
