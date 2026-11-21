//! Linear framebuffer surface: a `simple-framebuffer` left on by the
//! phone's bootloader, or QEMU's ramfb. 32-bit pixels.
use phone::ui::raster::Surface;

#[derive(Clone, Copy, Debug)]
pub struct Fb {
    pub base: usize,
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    /// true for x8b8g8r8 / a8b8g8r8 (red in the low byte)
    pub bgr: bool,
    /// clean D-cache after drawing (real display engines don't snoop)
    pub clean: bool,
}

impl Surface for Fb {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, rgb: u32) {
        let px = if self.bgr { (rgb & 0xFF) << 16 | (rgb & 0xFF00) | (rgb >> 16) & 0xFF } else { rgb } | 0xFF00_0000;
        let w = w.min(self.width.saturating_sub(x));
        for yy in y..(y + h).min(self.height) {
            let row = (self.base + yy * self.stride + x * 4) as *mut u32;
            for i in 0..w {
                unsafe { row.add(i).write_volatile(px) };
            }
        }
    }
    fn flush(&mut self, x: usize, y: usize, w: usize, h: usize) {
        if !self.clean {
            return;
        }
        for yy in y..(y + h).min(self.height) {
            crate::mmu::clean_dcache(self.base + yy * self.stride + x * 4, w * 4);
        }
    }
}
