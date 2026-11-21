//! 3GPP TS 23.038 GSM 7-bit default alphabet, extension table and septet packing.
use crate::prelude::*;

const BASIC: &str = "@£$¥èéùìòÇ\nØø\rÅåΔ_ΦΓΛΩΠΨΣΘΞ\u{1b}ÆæßÉ !\"#¤%&'()*+,-./0123456789:;<=>?¡ABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÑÜ§¿abcdefghijklmnopqrstuvwxyzäöñüà";

const EXT: &[(u8, char)] = &[
    (0x0A, '\u{0C}'),
    (0x14, '^'),
    (0x28, '{'),
    (0x29, '}'),
    (0x2F, '\\'),
    (0x3C, '['),
    (0x3D, '~'),
    (0x3E, ']'),
    (0x40, '|'),
    (0x65, '€'),
];

pub const ESC: u8 = 0x1B;

fn basic_index(c: char) -> Option<u8> {
    if c == '\u{1b}' {
        return None;
    }
    BASIC.chars().position(|x| x == c).map(|p| p as u8)
}

fn basic_char(i: u8) -> char {
    BASIC.chars().nth(i as usize).unwrap_or('?')
}

/// Encode text to septets. Returns `None` if a character is not representable.
pub fn encode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        if let Some(i) = basic_index(c) {
            out.push(i);
        } else if let Some(&(code, _)) = EXT.iter().find(|(_, e)| *e == c) {
            out.push(ESC);
            out.push(code);
        } else {
            return None;
        }
    }
    Some(out)
}

pub fn is_encodable(s: &str) -> bool {
    encode(s).is_some()
}

pub fn decode(septets: &[u8]) -> String {
    let mut out = String::with_capacity(septets.len());
    let mut i = 0;
    while i < septets.len() {
        let s = septets[i] & 0x7F;
        if s == ESC && i + 1 < septets.len() {
            let e = septets[i + 1] & 0x7F;
            match EXT.iter().find(|(code, _)| *code == e) {
                Some(&(_, c)) => out.push(c),
                None => out.push(' '),
            }
            i += 2;
        } else {
            out.push(basic_char(s));
            i += 1;
        }
    }
    out
}

/// Pack septets LSB-first starting at `bit_offset` bits into the output.
pub fn pack(septets: &[u8], bit_offset: usize) -> Vec<u8> {
    let total = bit_offset + septets.len() * 7;
    let mut out = vec![0u8; total.div_ceil(8)];
    for (i, &s) in septets.iter().enumerate() {
        let pos = bit_offset + i * 7;
        let v = ((s & 0x7F) as u16) << (pos % 8);
        out[pos / 8] |= v as u8;
        if v >> 8 != 0 {
            out[pos / 8 + 1] |= (v >> 8) as u8;
        }
    }
    out
}

pub fn unpack(data: &[u8], bit_offset: usize, count: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let pos = bit_offset + i * 7;
        let lo = *data.get(pos / 8).unwrap_or(&0) as u16;
        let hi = *data.get(pos / 8 + 1).unwrap_or(&0) as u16;
        out.push((((hi << 8) | lo) >> (pos % 8)) as u8 & 0x7F);
    }
    out
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn table_size() {
        assert_eq!(BASIC.chars().count(), 128);
        assert_eq!(basic_char(0x41), 'A');
        assert_eq!(basic_char(0x61), 'a');
        assert_eq!(basic_char(0x30), '0');
        assert_eq!(basic_char(0x7F), 'à');
    }
    #[test]
    fn pack_hello() {
        // Classic example: "hellohello" -> E8329BFD4697D9EC37
        let s = encode("hellohello").unwrap();
        assert_eq!(crate::util::hex(&pack(&s, 0)), "E8329BFD4697D9EC37");
        let u = unpack(&pack(&s, 0), 0, s.len());
        assert_eq!(decode(&u), "hellohello");
    }
    #[test]
    fn ext() {
        let s = encode("a€[b]").unwrap();
        assert_eq!(s.len(), 8);
        assert_eq!(decode(&s), "a€[b]");
        assert!(encode("日本").is_none());
    }
}
