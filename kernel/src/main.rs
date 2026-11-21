//! Tranquility kernel — a minimal aarch64 kernel for a dumbphone.
//!
//! Single core, single address space, event driven:
//!   * boot.rs   — reset, EL2→EL1, MMU (identity map), vector table
//!   * gic.rs    — GICv2 interrupt controller
//!   * timer.rs  — generic timer tick + monotonic clock
//!   * uart.rs   — PL011 console (display + keypad over the serial line)
//!   * heap.rs   — global allocator
//!   * keys.rs   — terminal bytes → keypad events
//!
//! The phone stack (`phone` crate) runs as the kernel's only task: the
//! main loop feeds it key events and clock ticks, lets it poll the
//! baseband, redraws the screen when it changes and sleeps in WFI until
//! the next interrupt.
#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
mod uart;
mod boot;
mod gic;
mod heap;
mod keys;
mod timer;

use alloc::boxed::Box;
use alloc::string::String;
use core::arch::asm;
use core::panic::PanicInfo;
use phone::sim::SimModem;
use phone::ui::Phone;

#[no_mangle]
pub extern "C" fn kmain() -> ! {
    uart::init();
    let (hs, he) = heap::init();
    uart::write("\x1b[2J\x1b[H");
    kprintln!("Tranquility kernel 0.1 (aarch64)");
    kprintln!("  EL{}  timer {} Hz  tick {} Hz", current_el(), timer::freq(), timer::HZ);
    kprintln!("  heap {:#x}..{:#x} ({} MiB)", hs, he, (he - hs) >> 20);
    gic::init();
    gic::enable(timer::IRQ);
    gic::enable(uart::IRQ);
    timer::init();
    uart::enable_rx_irq();
    unsafe { asm!("msr daifclr, #2") }; // unmask IRQs
    kprintln!("  gic, timer, uart irq: up");
    kprintln!("  baseband: simulated TQ-5G1 over AT link");
    kprintln!("");
    kprintln!("  SIM PIN is 1234");
    let t0 = timer::now_ms();
    while timer::now_ms() - t0 < 1500 {
        wfi();
    }

    let mut phone = Phone::new(Box::new(SimModem::new()), timer::now_ms);
    phone.boot();
    let mut dec = keys::Decoder::default();
    let mut shown = String::new();
    let mut last_draw = 0u64;
    uart::write("\x1b[2J\x1b[?25l");
    loop {
        let now = timer::now_ms();
        let mut pressed = false;
        while let Some(b) = uart::getc() {
            if let Some(k) = dec.feed(b, now) {
                phone.key(k);
                pressed = true;
            }
        }
        if let Some(k) = dec.flush(now) {
            phone.key(k);
            pressed = true;
        }
        phone.step(now);
        if pressed || now.saturating_sub(last_draw) >= 100 {
            last_draw = now;
            let s = phone.render_ansi();
            if s != shown {
                uart::write(&s);
                shown = s;
            }
        }
        wfi();
    }
}

fn wfi() {
    unsafe { asm!("wfi") };
}

fn current_el() -> u64 {
    let el: u64;
    unsafe { asm!("mrs {}, CurrentEL", out(reg) el) };
    el >> 2
}

#[no_mangle]
extern "C" fn irq_handler() {
    let iar = gic::ack();
    match iar & 0x3FF {
        timer::IRQ => timer::on_irq(),
        uart::IRQ => uart::on_irq(),
        1023 => return, // spurious
        _ => {}
    }
    gic::eoi(iar);
}

fn sysreg_dump(kind: &str) -> ! {
    let (esr, elr, far): (u64, u64, u64);
    unsafe {
        asm!("mrs {}, esr_el1", out(reg) esr);
        asm!("mrs {}, elr_el1", out(reg) elr);
        asm!("mrs {}, far_el1", out(reg) far);
    }
    panic!("{} exception: ESR={:#x} (EC={:#x}) ELR={:#x} FAR={:#x}", kind, esr, esr >> 26, elr, far);
}

#[no_mangle]
extern "C" fn sync_handler(_frame: *mut u64) {
    sysreg_dump("synchronous");
}

#[no_mangle]
extern "C" fn other_handler(_frame: *mut u64) {
    sysreg_dump("unexpected");
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe { asm!("msr daifset, #2") };
    uart::write("\x1b[?25h\r\n\r\n*** KERNEL PANIC ***\r\n");
    kprintln!("{}", info);
    kprintln!("ticks={} heap in use={} KiB", timer::ticks(), heap::IN_USE.load(core::sync::atomic::Ordering::Relaxed) >> 10);
    loop {
        unsafe { asm!("wfe") };
    }
}
