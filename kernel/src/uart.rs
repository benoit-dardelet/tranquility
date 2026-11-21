//! ARM PL011 UART driver (QEMU virt: 0x0900_0000, SPI 1 / INTID 33).
//! TX is polled; RX is interrupt driven into a lock-free SPSC ring.
use core::fmt;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicUsize, Ordering};

static BASE: AtomicUsize = AtomicUsize::new(0);
const DR: usize = 0x00;
const FR: usize = 0x18;
const IBRD: usize = 0x24;
const FBRD: usize = 0x28;
const LCRH: usize = 0x2C;
const CR: usize = 0x30;
const IMSC: usize = 0x38;
const ICR: usize = 0x44;
const FR_RXFE: u32 = 1 << 4;
const FR_TXFF: u32 = 1 << 5;
pub static IRQ: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(33);

fn reg(off: usize) -> *mut u32 {
    (BASE.load(Ordering::Relaxed) + off) as *mut u32
}

pub fn present() -> bool {
    BASE.load(Ordering::Relaxed) != 0
}

pub fn init(base: usize, irq: u32) {
    BASE.store(base, Ordering::Relaxed);
    IRQ.store(irq, Ordering::Relaxed);
    unsafe {
        write_volatile(reg(CR), 0);
        write_volatile(reg(ICR), 0x7FF);
        write_volatile(reg(IBRD), 13); // 115200 @ 24 MHz (ignored by QEMU)
        write_volatile(reg(FBRD), 1);
        write_volatile(reg(LCRH), 0x60); // 8N1, FIFOs off → one IRQ per byte
        write_volatile(reg(CR), 0x301); // UARTEN | TXE | RXE
    }
}

pub fn enable_rx_irq() {
    unsafe { write_volatile(reg(IMSC), (1 << 4) | (1 << 6)) } // RXIM | RTIM
}

pub fn putc(b: u8) {
    if !present() {
        return;
    }
    unsafe {
        while read_volatile(reg(FR)) & FR_TXFF != 0 {
            core::hint::spin_loop();
        }
        write_volatile(reg(DR), b as u32);
    }
}

pub fn write(s: &str) {
    for b in s.bytes() {
        putc(b);
    }
}

// ---------------------------------------------------------------- RX ring

const RING: usize = 1024;
static mut RX: [u8; RING] = [0; RING];
static HEAD: AtomicUsize = AtomicUsize::new(0); // written by IRQ
static TAIL: AtomicUsize = AtomicUsize::new(0); // written by main loop

/// Interrupt handler: drain the receive FIFO into the ring.
pub fn on_irq() {
    if !present() {
        return;
    }
    unsafe {
        while read_volatile(reg(FR)) & FR_RXFE == 0 {
            let b = read_volatile(reg(DR)) as u8;
            let h = HEAD.load(Ordering::Relaxed);
            let n = (h + 1) % RING;
            if n != TAIL.load(Ordering::Acquire) {
                (*core::ptr::addr_of_mut!(RX))[h] = b;
                HEAD.store(n, Ordering::Release);
            }
        }
        write_volatile(reg(ICR), (1 << 4) | (1 << 6));
    }
}

pub fn getc() -> Option<u8> {
    let t = TAIL.load(Ordering::Relaxed);
    if t == HEAD.load(Ordering::Acquire) {
        return None;
    }
    let b = unsafe { (*core::ptr::addr_of!(RX))[t] };
    TAIL.store((t + 1) % RING, Ordering::Release);
    Some(b)
}

pub struct Writer;

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                putc(b'\r');
            }
            putc(b);
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! kprintln {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let _ = writeln!($crate::uart::Writer, $($arg)*);
    }};
}
