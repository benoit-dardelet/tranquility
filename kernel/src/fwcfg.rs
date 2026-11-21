//! QEMU fw_cfg (MMIO) — only used to configure the `ramfb` display.
use core::ptr::{read_volatile, write_volatile};

const SEL_FILE_DIR: u16 = 0x19;
const DMA_ERROR: u32 = 1;
const DMA_SELECT: u32 = 8;
const DMA_WRITE: u32 = 0x10;

fn select(base: usize, key: u16) {
    unsafe { write_volatile((base + 8) as *mut u16, key.to_be()) }
}

fn read_bytes(base: usize, out: &mut [u8]) {
    for b in out.iter_mut() {
        *b = unsafe { read_volatile(base as *const u8) };
    }
}

fn find_file(base: usize, name: &str) -> Option<u16> {
    select(base, SEL_FILE_DIR);
    let mut n = [0u8; 4];
    read_bytes(base, &mut n);
    for _ in 0..u32::from_be_bytes(n).min(256) {
        let mut e = [0u8; 64];
        read_bytes(base, &mut e);
        let sel = u16::from_be_bytes([e[4], e[5]]);
        let nm = &e[8..];
        let end = nm.iter().position(|&b| b == 0).unwrap_or(nm.len());
        if &nm[..end] == name.as_bytes() {
            return Some(sel);
        }
    }
    None
}

#[repr(C, align(16))]
struct DmaAccess {
    control: u32,
    length: u32,
    address: u64,
}

fn dma_write(base: usize, sel: u16, data: &[u8]) -> bool {
    let mut acc = DmaAccess { control: ((sel as u32) << 16 | DMA_SELECT | DMA_WRITE).to_be(), length: (data.len() as u32).to_be(), address: (data.as_ptr() as u64).to_be() };
    let a = &mut acc as *mut DmaAccess as u64;
    unsafe {
        core::arch::asm!("dsb sy");
        write_volatile((base + 16) as *mut u32, ((a >> 32) as u32).to_be());
        write_volatile((base + 20) as *mut u32, (a as u32).to_be());
        for _ in 0..10_000_000 {
            let c = u32::from_be(read_volatile(&acc.control));
            if c & !DMA_ERROR == 0 {
                return c & DMA_ERROR == 0;
            }
        }
    }
    false
}

/// Point QEMU's ramfb at `fb` (XRGB8888). Returns false if no ramfb device.
pub fn setup_ramfb(base: usize, fb: usize, w: u32, h: u32) -> bool {
    let sel = match find_file(base, "etc/ramfb") {
        Some(s) => s,
        None => return false,
    };
    let mut cfg = [0u8; 28];
    cfg[0..8].copy_from_slice(&(fb as u64).to_be_bytes());
    cfg[8..12].copy_from_slice(&0x3432_5258u32.to_be_bytes()); // 'XR24'
    cfg[12..16].copy_from_slice(&0u32.to_be_bytes());
    cfg[16..20].copy_from_slice(&w.to_be_bytes());
    cfg[20..24].copy_from_slice(&h.to_be_bytes());
    cfg[24..28].copy_from_slice(&(w * 4).to_be_bytes());
    dma_write(base, sel, &cfg)
}
