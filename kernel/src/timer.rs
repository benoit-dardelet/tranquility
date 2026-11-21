//! ARM generic timer: EL1 *virtual* timer as the system tick (works under
//! a hypervisor, as on Qualcomm phones), CNTVCT for the monotonic clock.
use core::arch::asm;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const HZ: u64 = 100;
static TICKS: AtomicU64 = AtomicU64::new(0);
pub static IRQ: AtomicU32 = AtomicU32::new(27); // PPI 11 on QEMU; from DTB otherwise

pub fn freq() -> u64 {
    let f: u64;
    unsafe { asm!("mrs {}, cntfrq_el0", out(reg) f) };
    f.max(1)
}

pub fn counter() -> u64 {
    let c: u64;
    unsafe { asm!("isb", "mrs {}, cntvct_el0", out(reg) c) };
    c
}

/// Milliseconds since boot (well, since the counter started).
pub fn now_ms() -> u64 {
    counter() / (freq() / 1000).max(1)
}

fn arm() {
    let tval = freq() / HZ;
    unsafe {
        asm!("msr cntv_tval_el0, {}", in(reg) tval);
        asm!("msr cntv_ctl_el0, {}", in(reg) 1u64);
    }
}

pub fn init(irq: u32) {
    IRQ.store(irq, Ordering::Relaxed);
    arm();
}

pub fn on_irq() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    arm();
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Busy-wait helper for polling mode.
pub fn delay_ms(ms: u64) {
    let end = now_ms() + ms;
    while now_ms() < end {
        core::hint::spin_loop();
    }
}
