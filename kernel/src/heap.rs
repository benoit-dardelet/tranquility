//! Kernel heap: power-of-two size-class allocator over a RAM region picked
//! at boot from the device tree. Freed blocks go to per-class free lists;
//! fresh blocks are carved from a bump pointer.
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const CLASSES: usize = 28;

struct Inner {
    next: usize,
    end: usize,
    free: [usize; CLASSES],
}

pub struct Heap {
    lock: AtomicBool,
    inner: UnsafeCell<Inner>,
}

unsafe impl Sync for Heap {}

pub static IN_USE: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
pub static HEAP: Heap = Heap { lock: AtomicBool::new(false), inner: UnsafeCell::new(Inner { next: 0, end: 0, free: [0; CLASSES] }) };

pub fn init(start: usize, end: usize) {
    let h = unsafe { &mut *HEAP.inner.get() };
    h.next = (start + 4095) & !4095;
    h.end = end;
}

pub fn range() -> (usize, usize) {
    let h = unsafe { &*HEAP.inner.get() };
    (h.next, h.end)
}

fn class(l: &Layout) -> usize {
    let s = l.size().max(l.align()).max(16).next_power_of_two();
    s.trailing_zeros() as usize - 4
}

impl Heap {
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        let r = f(unsafe { &mut *self.inner.get() });
        self.lock.store(false, Ordering::Release);
        r
    }
}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let c = class(&l);
        if c >= CLASSES {
            return core::ptr::null_mut();
        }
        let size = 16usize << c;
        let p = self.with(|h| {
            let head = h.free[c];
            if head != 0 {
                h.free[c] = *(head as *const usize);
                return head;
            }
            let align = size.min(4096).max(l.align());
            let p = (h.next + align - 1) & !(align - 1);
            if p + size > h.end {
                return 0;
            }
            h.next = p + size;
            p
        });
        if p != 0 {
            IN_USE.fetch_add(size, Ordering::Relaxed);
        }
        p as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, l: Layout) {
        let c = class(&l);
        IN_USE.fetch_sub(16usize << c, Ordering::Relaxed);
        self.with(|h| {
            *(ptr as *mut usize) = h.free[c];
            h.free[c] = ptr as usize;
        });
    }
}
