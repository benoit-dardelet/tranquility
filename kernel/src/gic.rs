//! GICv2 distributor + CPU interface (QEMU virt: GICD 0x0800_0000, GICC 0x0801_0000).
use core::ptr::{read_volatile, write_volatile};

const GICD: usize = 0x0800_0000;
const GICC: usize = 0x0801_0000;

unsafe fn w(addr: usize, v: u32) {
    write_volatile(addr as *mut u32, v)
}
unsafe fn r(addr: usize) -> u32 {
    read_volatile(addr as *const u32)
}

pub fn init() {
    unsafe {
        w(GICD, 0); // disable while configuring
        let lines = ((r(GICD + 0x4) & 0x1F) + 1) * 32;
        for i in (32..lines).step_by(32) {
            w(GICD + 0x180 + (i / 8) as usize, 0xFFFF_FFFF); // ICENABLER
        }
        for i in (0..lines).step_by(4) {
            w(GICD + 0x400 + i as usize, 0xA0A0_A0A0); // IPRIORITYR
        }
        for i in (32..lines).step_by(4) {
            w(GICD + 0x800 + i as usize, 0x0101_0101); // ITARGETSR → CPU0
        }
        w(GICD, 1);
        w(GICC + 0x4, 0xF0); // PMR: allow priorities < 0xF0
        w(GICC, 1);
    }
}

pub fn enable(irq: u32) {
    unsafe { w(GICD + 0x100 + (irq / 32 * 4) as usize, 1 << (irq % 32)) }
}

pub fn ack() -> u32 {
    unsafe { r(GICC + 0xC) }
}

pub fn eoi(iar: u32) {
    unsafe { w(GICC + 0x10, iar) }
}
