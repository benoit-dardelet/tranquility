//! Updates to the boot identity map: mark RAM (from the DTB) as normal
//! cacheable memory, one 1 GiB block at a time, with break-before-make.
use core::arch::asm;

extern "C" {
    static mut __l1_table: [u64; 512];
}

const NORMAL: u64 = 0x705;

pub fn map_normal(base: u64, size: u64, avoid: &[(u64, u64)]) {
    if size == 0 {
        return;
    }
    let first = base >> 30;
    let last = (base + size - 1) >> 30;
    for gb in first..=last.min(511) {
        let (s, e) = (gb << 30, (gb + 1) << 30);
        // never turn a device window into cacheable memory
        if avoid.iter().any(|&(b, l)| b < e && b + l > s) {
            continue;
        }
        unsafe {
            let t = core::ptr::addr_of_mut!(__l1_table) as *mut u64;
            let want = (gb << 30) | NORMAL;
            if t.add(gb as usize).read_volatile() == want {
                continue;
            }
            t.add(gb as usize).write_volatile(0);
            asm!("dsb ishst", "tlbi vmalle1", "dsb ish", "isb");
            t.add(gb as usize).write_volatile(want);
            asm!("dsb ishst", "isb");
        }
    }
}

/// Clean data cache lines covering [addr, addr+len) to the point of coherency
/// so a scanning display engine sees our framebuffer writes.
pub fn clean_dcache(addr: usize, len: usize) {
    let line = 64;
    let mut a = addr & !(line - 1);
    while a < addr + len {
        unsafe { asm!("dc cvac, {}", in(reg) a) };
        a += line;
    }
    unsafe { asm!("dsb sy") };
}
