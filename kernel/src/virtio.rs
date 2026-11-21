//! virtio-mmio (version 2 / "modern") transport and split virtqueues.
//! All drivers poll for completions; no virtio interrupts are needed.
use alloc::alloc::{alloc_zeroed, Layout};
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

pub const DEV_BLOCK: u32 = 2;
pub const DEV_INPUT: u32 = 18;
pub const DEV_SOUND: u32 = 25;

const STATUS_ACK: u32 = 1;
const STATUS_DRIVER: u32 = 2;
const STATUS_DRIVER_OK: u32 = 4;
const STATUS_FEATURES_OK: u32 = 8;

pub const F_NEXT: u16 = 1;
pub const F_WRITE: u16 = 2;

pub struct Mmio {
    pub base: usize,
}

impl Mmio {
    pub fn r(&self, off: usize) -> u32 {
        unsafe { read_volatile((self.base + off) as *const u32) }
    }
    pub fn w(&self, off: usize, v: u32) {
        unsafe { write_volatile((self.base + off) as *mut u32, v) }
    }
    pub fn cfg32(&self, off: usize) -> u32 {
        self.r(0x100 + off)
    }
    pub fn cfg64(&self, off: usize) -> u64 {
        self.cfg32(off) as u64 | (self.cfg32(off + 4) as u64) << 32
    }

    /// Returns the device id if a modern virtio device lives here.
    pub fn probe(base: usize) -> Option<(Mmio, u32)> {
        let m = Mmio { base };
        if m.r(0) != 0x7472_6976 || m.r(4) != 2 {
            return None;
        }
        let id = m.r(8);
        (id != 0).then_some((m, id))
    }

    /// Reset and negotiate VIRTIO_F_VERSION_1 only.
    pub fn begin(&self) -> bool {
        self.w(0x70, 0);
        self.w(0x70, STATUS_ACK);
        self.w(0x70, STATUS_ACK | STATUS_DRIVER);
        self.w(0x14, 1);
        let hi = self.r(0x10);
        if hi & 1 == 0 {
            return false;
        }
        self.w(0x24, 0);
        self.w(0x20, 0);
        self.w(0x24, 1);
        self.w(0x20, 1);
        self.w(0x70, STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK);
        self.r(0x70) & STATUS_FEATURES_OK != 0
    }

    pub fn finish(&self) {
        self.w(0x70, STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK);
    }

    pub fn queue(&self, idx: u32, want: u16) -> Option<Queue> {
        self.w(0x30, idx);
        let max = self.r(0x34) as u16;
        if max == 0 {
            return None;
        }
        let n = want.min(max);
        let q = Queue::new(n, idx);
        self.w(0x38, n as u32);
        let (d, a, u) = (q.desc as u64, q.avail as u64, q.used as u64);
        self.w(0x80, d as u32);
        self.w(0x84, (d >> 32) as u32);
        self.w(0x90, a as u32);
        self.w(0x94, (a >> 32) as u32);
        self.w(0xA0, u as u32);
        self.w(0xA4, (u >> 32) as u32);
        self.w(0x44, 1);
        Some(q)
    }

    pub fn notify(&self, q: &Queue) {
        fence(Ordering::SeqCst);
        self.w(0x50, q.index);
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Desc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

pub struct Queue {
    pub size: u16,
    pub index: u32,
    desc: *mut Desc,
    avail: *mut u16,
    used: *mut u8,
    free: alloc::vec::Vec<u16>,
    last_used: u16,
}

fn page(bytes: usize) -> *mut u8 {
    unsafe { alloc_zeroed(Layout::from_size_align(bytes.max(4096).next_power_of_two(), 4096).unwrap()) }
}

/// A DMA-able buffer (identity mapped, so the VA is the bus address).
pub fn dma(bytes: usize) -> *mut u8 {
    page(bytes)
}

impl Queue {
    fn new(size: u16, index: u32) -> Queue {
        let n = size as usize;
        Queue {
            size,
            index,
            desc: page(16 * n) as *mut Desc,
            avail: page(6 + 2 * n) as *mut u16,
            used: page(6 + 8 * n),
            free: (0..size).rev().collect(),
            last_used: 0,
        }
    }

    /// Queue a descriptor chain: (address, length, device-writable).
    pub fn submit(&mut self, bufs: &[(u64, u32, bool)]) -> Option<u16> {
        if self.free.len() < bufs.len() || bufs.is_empty() {
            return None;
        }
        let ids: alloc::vec::Vec<u16> = (0..bufs.len()).map(|_| self.free.pop().unwrap()).collect();
        for (i, &(addr, len, write)) in bufs.iter().enumerate() {
            let mut flags = if write { F_WRITE } else { 0 };
            if i + 1 < bufs.len() {
                flags |= F_NEXT;
            }
            let next = if i + 1 < bufs.len() { ids[i + 1] } else { 0 };
            unsafe { self.desc.add(ids[i] as usize).write_volatile(Desc { addr, len, flags, next }) };
        }
        unsafe {
            let idx = self.avail.add(1).read_volatile();
            self.avail.add(2 + (idx % self.size) as usize).write_volatile(ids[0]);
            fence(Ordering::SeqCst);
            self.avail.add(1).write_volatile(idx.wrapping_add(1));
        }
        Some(ids[0])
    }

    /// Pop one completed chain: (head id, bytes written by the device).
    pub fn pop(&mut self) -> Option<(u16, u32)> {
        fence(Ordering::SeqCst);
        let used_idx = unsafe { (self.used.add(2) as *const u16).read_volatile() };
        if used_idx == self.last_used {
            return None;
        }
        let slot = (self.last_used % self.size) as usize;
        let (id, len) = unsafe {
            let e = self.used.add(4 + slot * 8);
            ((e as *const u32).read_volatile() as u16, (e.add(4) as *const u32).read_volatile())
        };
        self.last_used = self.last_used.wrapping_add(1);
        // free the chain
        let mut d = id;
        loop {
            self.free.push(d);
            let desc = unsafe { self.desc.add(d as usize).read_volatile() };
            if desc.flags & F_NEXT == 0 {
                break;
            }
            d = desc.next;
        }
        Some((id, len))
    }
}
