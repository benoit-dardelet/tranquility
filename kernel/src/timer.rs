//! ARM generic timer: EL1 physical timer (PPI 14 / INTID 30) as the
//! system tick, CNTPCT for the monotonic clock.
use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};

pub const IRQ: u32 = 30;
pub const HZ: u64 = 100;
static TICKS: AtomicU64 = AtomicU64::new(0);

pub fn freq() -> u64 {
    let f: u64;
    unsafe { asm!("mrs {}, cntfrq_el0", out(reg) f) };
    f
}

pub fn counter() -> u64 {
    let c: u64;
    unsafe { asm!("isb", "mrs {}, cntpct_el0", out(reg) c) };
    c
}

/// Milliseconds since boot.
pub fn now_ms() -> u64 {
    counter() / (freq() / 1000).max(1)
}

fn arm() {
    let tval = freq() / HZ;
    unsafe {
        asm!("msr cntp_tval_el0, {}", in(reg) tval);
        asm!("msr cntp_ctl_el0, {}", in(reg) 1u64);
    }
}

pub fn init() {
    arm();
}

pub fn on_irq() {
    TICKS.fetch_add(1, Ordering::Relaxed);
    arm();
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}
