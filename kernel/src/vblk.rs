//! virtio-blk driver: synchronous sector reads/writes, used as the phone's
//! persistent storage (`phone::store::BlockDev`).
use crate::virtio::{dma, Mmio, Queue};
use phone::store::{BlockDev, SECTOR};

pub struct VirtioBlk {
    m: Mmio,
    q: Queue,
    hdr: *mut u8,
    data: *mut u8,
    status: *mut u8,
    sectors: u64,
}

impl VirtioBlk {
    pub fn new(m: Mmio) -> Option<VirtioBlk> {
        if !m.begin() {
            return None;
        }
        let q = m.queue(0, 8)?;
        let sectors = m.cfg64(0);
        m.finish();
        Some(VirtioBlk { m, q, hdr: dma(16), data: dma(SECTOR), status: dma(1), sectors })
    }

    fn io(&mut self, write: bool, lba: u64) -> bool {
        if lba >= self.sectors {
            return false;
        }
        unsafe {
            (self.hdr as *mut u32).write_volatile(write as u32); // 0 = IN, 1 = OUT
            (self.hdr.add(4) as *mut u32).write_volatile(0);
            (self.hdr.add(8) as *mut u64).write_volatile(lba);
            self.status.write_volatile(0xFF);
        }
        let bufs = [(self.hdr as u64, 16, false), (self.data as u64, SECTOR as u32, !write), (self.status as u64, 1, true)];
        if self.q.submit(&bufs).is_none() {
            return false;
        }
        self.m.notify(&self.q);
        let start = crate::timer::now_ms();
        loop {
            if self.q.pop().is_some() {
                break;
            }
            if crate::timer::now_ms() - start > 2000 {
                return false;
            }
            core::hint::spin_loop();
        }
        unsafe { self.status.read_volatile() == 0 }
    }
}

impl BlockDev for VirtioBlk {
    fn sectors(&self) -> u64 {
        self.sectors
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> bool {
        for (i, chunk) in buf.chunks_mut(SECTOR).enumerate() {
            if !self.io(false, lba + i as u64) {
                return false;
            }
            unsafe { core::ptr::copy_nonoverlapping(self.data, chunk.as_mut_ptr(), chunk.len()) };
        }
        true
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> bool {
        for (i, chunk) in buf.chunks(SECTOR).enumerate() {
            unsafe {
                core::ptr::write_bytes(self.data, 0, SECTOR);
                core::ptr::copy_nonoverlapping(chunk.as_ptr(), self.data, chunk.len());
            }
            if !self.io(true, lba + i as u64) {
                return false;
            }
        }
        true
    }
}
