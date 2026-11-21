//! Pixel rendering of a [`Frame`] for framebuffer displays: the same
//! layout as the ANSI terminal view, drawn with the 8x16 bitmap font,
//! scaled to fit, redrawing only cells that changed.
use super::font;
use super::{center, pad, Frame, ROWS, W};
use crate::prelude::*;
use crate::util::char_width;

pub const COLS: usize = W + 2;
pub const LINES: usize = ROWS + 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub inv: bool,
    pub accent: bool,
}

const BLANK: Cell = Cell { ch: ' ', inv: false, accent: false };

fn put(row: &mut Vec<Cell>, s: &str, inv: bool, accent: bool) {
    for ch in s.chars() {
        match char_width(ch) {
            0 => {}
            2 => {
                row.push(Cell { ch, inv, accent });
                row.push(Cell { ch: '\0', inv, accent });
            }
            _ => row.push(Cell { ch, inv, accent }),
        }
    }
}

/// Lay a frame out as COLS x LINES cells (row-major).
pub fn cells(f: &Frame) -> Vec<Cell> {
    let bar = "─".repeat(W);
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut row = |left: &str, mid: &str, right: &str, inv: bool, accent: bool| {
        let mut r = Vec::with_capacity(COLS);
        put(&mut r, left, false, false);
        put(&mut r, mid, inv, accent);
        put(&mut r, right, false, false);
        r.resize(COLS, BLANK);
        r.truncate(COLS);
        rows.push(r);
    };
    row("┌", &bar, "┐", false, false);
    row("│", &pad(&f.status, W), "│", true, false);
    row("│", &center(&f.title, W), "│", false, true);
    row("├", &bar, "┤", false, false);
    for i in 0..ROWS {
        let l = f.lines.get(i).map(|s| s.as_str()).unwrap_or("");
        row("│", &pad(l, W), "│", f.sel == Some(i), false);
    }
    row("├", &bar, "┤", false, false);
    let lw = W / 3;
    let r = &f.soft[2];
    let right = format!("{}{}", " ".repeat(lw.saturating_sub(crate::util::char_len(r))), crate::util::trunc(r, lw));
    let soft = format!("{}{}{}", pad(&f.soft[0], lw), center(&f.soft[1], W - 2 * lw), right);
    row("│", &soft, "│", false, true);
    row("└", &bar, "┘", false, false);
    rows.concat()
}

pub trait Surface {
    fn size(&self) -> (usize, usize);
    /// Fill a rectangle with 0xRRGGBB.
    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, rgb: u32);
    /// Called after a batch of drawing (flush caches, present...).
    fn flush(&mut self, _x: usize, _y: usize, _w: usize, _h: usize) {}
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub body: u32,
    pub bg: u32,
    pub fg: u32,
    pub accent: u32,
}

/// Classic green-grey LCD look.
pub const LCD: Theme = Theme { body: 0x1A1C1A, bg: 0xB7C8A0, fg: 0x1C2616, accent: 0x0E3B2E };

pub struct Raster {
    prev: Vec<Cell>,
    pub scale: usize,
    pub ox: usize,
    pub oy: usize,
    pub theme: Theme,
    cleared: bool,
}

impl Raster {
    pub fn new(w: usize, h: usize) -> Raster {
        let scale = (w / (COLS * 8)).min(h / (LINES * 16)).max(1);
        let ox = w.saturating_sub(COLS * 8 * scale) / 2;
        let oy = h.saturating_sub(LINES * 16 * scale) / 2;
        Raster { prev: Vec::new(), scale, ox, oy, theme: LCD, cleared: false }
    }

    fn cell(&self, s: &mut dyn Surface, i: usize, c: Cell) {
        let (cx, cy) = (i % COLS, i / COLS);
        let sc = self.scale;
        let (x0, y0) = (self.ox + cx * 8 * sc, self.oy + cy * 16 * sc);
        let (mut fg, mut bg) = (if c.accent { self.theme.accent } else { self.theme.fg }, self.theme.bg);
        if c.inv {
            core::mem::swap(&mut fg, &mut bg);
        }
        let g = if c.ch == '\0' { &[0u8; 16] } else { font::glyph(c.ch) };
        for (r, bits) in g.iter().enumerate() {
            // run-length encode each glyph row
            let mut x = 0;
            while x < 8 {
                let on = bits & (0x80 >> x) != 0;
                let mut n = 1;
                while x + n < 8 && (bits & (0x80 >> (x + n)) != 0) == on {
                    n += 1;
                }
                s.fill(x0 + x * sc, y0 + r * sc, n * sc, sc, if on { fg } else { bg });
                x += n;
            }
        }
        s.flush(x0, y0, 8 * sc, 16 * sc);
    }

    /// Draw a frame, touching only the cells that changed since last time.
    pub fn draw(&mut self, f: &Frame, s: &mut dyn Surface) -> usize {
        if !self.cleared {
            let (w, h) = s.size();
            s.fill(0, 0, w, h, self.theme.body);
            s.flush(0, 0, w, h);
            self.cleared = true;
        }
        let cells = cells(f);
        let mut n = 0;
        for (i, c) in cells.iter().enumerate() {
            if self.prev.get(i) != Some(c) {
                self.cell(s, i, *c);
                n += 1;
            }
        }
        self.prev = cells;
        n
    }

    /// Write raw text (e.g. a panic message) in the given colour, no diffing.
    pub fn text(&mut self, s: &mut dyn Surface, col: usize, row: usize, t: &str, inv: bool) {
        for (k, ch) in t.chars().enumerate() {
            let i = row * COLS + col + k;
            if col + k >= COLS {
                break;
            }
            self.cell(s, i, Cell { ch, inv, accent: false });
        }
        self.prev.clear();
    }
}

/// In-memory RGB surface (tests, screenshots).
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Image {
    pub fn new(w: usize, h: usize) -> Image {
        Image { w, h, px: vec![0; w * h] }
    }
    /// Binary PPM (P6).
    pub fn ppm(&self) -> Vec<u8> {
        let mut out = format!("P6\n{} {}\n255\n", self.w, self.h).into_bytes();
        for p in &self.px {
            out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
        }
        out
    }
}

impl Surface for Image {
    fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }
    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, rgb: u32) {
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                self.px[yy * self.w + xx] = rgb;
            }
        }
    }
}
