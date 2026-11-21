//! Persistent storage in a RAM region that survives a warm restart (the
//! region phones reserve for Linux "ramoops" crash logs). Contents are lost
//! on power-off; the A/B CRC-checked format above it then simply finds no
//! valid snapshot and the phone starts fresh.
use phone::store::{BlockDev, SECTOR};

pub struct RamStore {
    base: usize,
    sectors: u64,
}

impl RamStore {
    pub fn new(base: usize, size: usize) -> RamStore {
        RamStore { base, sectors: (size / SECTOR) as u64 }
    }
}

impl BlockDev for RamStore {
    fn sectors(&self) -> u64 {
        self.sectors
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> bool {
        if lba + (buf.len() / SECTOR) as u64 > self.sectors {
            return false;
        }
        let src = self.base + lba as usize * SECTOR;
        for (i, b) in buf.iter_mut().enumerate() {
            *b = unsafe { core::ptr::read_volatile((src + i) as *const u8) };
        }
        true
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> bool {
        if lba + (buf.len() / SECTOR) as u64 > self.sectors {
            return false;
        }
        let dst = self.base + lba as usize * SECTOR;
        for (i, &b) in buf.iter().enumerate() {
            unsafe { core::ptr::write_volatile((dst + i) as *mut u8, b) };
        }
        // push it out of the CPU caches to DRAM, or a reset would lose it
        crate::mmu::clean_dcache(dst, buf.len());
        true
    }
}
