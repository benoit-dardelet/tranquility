//! Monochrome BMP (Windows bitmap, 1 bpp) encode/decode — MMS picture
//! attachments that a text-mode dumbphone can still show as block art.
use crate::prelude::*;

pub fn encode(rows: &[&str]) -> Vec<u8> {
    let h = rows.len() as u32;
    let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as u32;
    let stride = (w.div_ceil(8)).div_ceil(4) * 4;
    let img_size = stride * h;
    let off = 14 + 40 + 8;
    let mut b = Vec::with_capacity((off + img_size) as usize);
    b.extend_from_slice(b"BM");
    b.extend_from_slice(&(off + img_size).to_le_bytes());
    b.extend_from_slice(&[0, 0, 0, 0]);
    b.extend_from_slice(&off.to_le_bytes());
    b.extend_from_slice(&40u32.to_le_bytes());
    b.extend_from_slice(&w.to_le_bytes());
    b.extend_from_slice(&h.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&img_size.to_le_bytes());
    b.extend_from_slice(&[0x13, 0x0B, 0, 0, 0x13, 0x0B, 0, 0]);
    b.extend_from_slice(&2u32.to_le_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0]); // palette: 0 = white, 1 = black
    for row in rows.iter().rev() {
        let mut line = vec![0u8; stride as usize];
        for (x, c) in row.chars().enumerate() {
            if c != ' ' && c != '.' {
                line[x / 8] |= 0x80 >> (x % 8);
            }
        }
        b.extend(line);
    }
    b
}

fn u32le(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
}

/// Render a 1-bpp BMP as text rows ('█' for ink).
pub fn render(d: &[u8]) -> Option<Vec<String>> {
    if d.get(0..2)? != b"BM" {
        return None;
    }
    let off = u32le(d, 10)? as usize;
    let w = u32le(d, 18)? as usize;
    let hraw = u32le(d, 22)? as i32;
    let bpp = u16::from_le_bytes(d.get(28..30)?.try_into().ok()?);
    if bpp != 1 || w > 64 {
        return None;
    }
    let h = hraw.unsigned_abs() as usize;
    // palette index of the darker colour
    let p0 = d.get(54..57)?.iter().map(|&x| x as u32).sum::<u32>();
    let p1 = d.get(58..61)?.iter().map(|&x| x as u32).sum::<u32>();
    let ink = if p1 < p0 { 1 } else { 0 };
    let stride = w.div_ceil(8).div_ceil(4) * 4;
    let mut rows = Vec::new();
    for y in 0..h {
        let src = if hraw > 0 { h - 1 - y } else { y };
        let line = d.get(off + src * stride..off + (src + 1) * stride)?;
        let s: String = (0..w).map(|x| if ((line[x / 8] >> (7 - x % 8)) & 1) == ink { '█' } else { ' ' }).collect();
        rows.push(s);
    }
    Some(rows)
}

pub const SMILEY: [&str; 8] = ["  ######  ", " #      # ", "#  #  #  #", "#        #", "# #    # #", "#  ####  #", " #      # ", "  ######  "];
pub const HEART: [&str; 7] = [" ##   ## ", "#### ####", "#########", " ####### ", "  #####  ", "   ###   ", "    #    "];

#[cfg(test)]
mod t {
    #[test]
    fn rt() {
        let b = super::encode(&super::SMILEY);
        let r = super::render(&b).unwrap();
        assert_eq!(r.len(), 8);
        assert_eq!(r[0], "  ██████  ");
    }
}
