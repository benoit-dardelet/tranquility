//! PSCI firmware calls: system restart and power off.
use core::arch::asm;

pub const SYSTEM_OFF: u64 = 0x8400_0008;
pub const SYSTEM_RESET: u64 = 0x8400_0009;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conduit {
    Smc,
    Hvc,
}

pub fn call(c: Conduit, func: u64) -> i64 {
    let mut x0 = func;
    unsafe {
        match c {
            Conduit::Smc => asm!("smc #0", inout("x0") x0, in("x1") 0u64, in("x2") 0u64, in("x3") 0u64),
            Conduit::Hvc => asm!("hvc #0", inout("x0") x0, in("x1") 0u64, in("x2") 0u64, in("x3") 0u64),
        }
    }
    x0 as i64
}
