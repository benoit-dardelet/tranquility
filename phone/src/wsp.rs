//! WAP-230 WSP binary encoding primitives (used by OMA MMS encapsulation and WAP Push).
use crate::prelude::*;

/// Well-known content types (WSP Appendix A, table 40) that we use.
pub const CONTENT_TYPES: &[(u8, &str)] = &[
    (0x00, "*/*"),
    (0x01, "text/*"),
    (0x02, "text/html"),
    (0x03, "text/plain"),
    (0x06, "text/x-vcalendar"),
    (0x07, "text/x-vcard"),
    (0x08, "text/vnd.wap.wml"),
    (0x1C, "image/*"),
    (0x1D, "image/gif"),
    (0x1E, "image/jpeg"),
    (0x1F, "image/tiff"),
    (0x20, "image/png"),
    (0x21, "image/vnd.wap.wbmp"),
    (0x22, "application/vnd.wap.multipart.*"),
    (0x23, "application/vnd.wap.multipart.mixed"),
    (0x33, "application/vnd.wap.multipart.related"),
    (0x3E, "application/vnd.wap.mms-message"),
];

pub fn content_type_code(s: &str) -> Option<u8> {
    CONTENT_TYPES.iter().find(|(_, n)| n.eq_ignore_ascii_case(s)).map(|(c, _)| *c)
}

pub fn content_type_name(c: u8) -> String {
    CONTENT_TYPES.iter().find(|(x, _)| *x == c).map(|(_, n)| n.to_string()).unwrap_or_else(|| format!("application/x-wsp-{:02x}", c))
}

#[derive(Default, Clone, Debug)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Writer {
        Writer { buf: Vec::new() }
    }
    pub fn byte(&mut self, b: u8) -> &mut Self {
        self.buf.push(b);
        self
    }
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(b);
        self
    }
    pub fn uintvar(&mut self, v: u32) -> &mut Self {
        self.buf.extend(uintvar(v));
        self
    }
    /// Short-integer: values 0..127 encoded with the high bit set.
    pub fn short_int(&mut self, v: u8) -> &mut Self {
        self.buf.push(v | 0x80);
        self
    }
    pub fn long_int(&mut self, v: u64) -> &mut Self {
        let bytes = v.to_be_bytes();
        let first = bytes.iter().position(|&b| b != 0).unwrap_or(7);
        self.buf.push((8 - first) as u8);
        self.buf.extend_from_slice(&bytes[first..]);
        self
    }
    /// Integer-value: short if < 128, else long.
    pub fn integer(&mut self, v: u64) -> &mut Self {
        if v < 128 {
            self.short_int(v as u8)
        } else {
            self.long_int(v)
        }
    }
    /// Text-string (null terminated, quoted if the first octet is >= 128).
    pub fn text(&mut self, s: &str) -> &mut Self {
        if s.as_bytes().first().is_some_and(|&b| b >= 0x80) {
            self.buf.push(0x7F);
        }
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
        self
    }
    pub fn quoted(&mut self, s: &str) -> &mut Self {
        self.buf.push(b'"');
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
        self
    }
    pub fn value_length(&mut self, len: usize) -> &mut Self {
        if len <= 30 {
            self.buf.push(len as u8);
        } else {
            self.buf.push(31);
            self.buf.extend(uintvar(len as u32));
        }
        self
    }
    /// Value-length prefixed block.
    pub fn with_length(&mut self, f: impl FnOnce(&mut Writer)) -> &mut Self {
        let mut w = Writer::new();
        f(&mut w);
        self.value_length(w.buf.len());
        self.buf.extend(w.buf);
        self
    }
}

pub fn uintvar(mut v: u32) -> Vec<u8> {
    let mut tmp = vec![(v & 0x7F) as u8];
    v >>= 7;
    while v > 0 {
        tmp.push((v & 0x7F) as u8 | 0x80);
        v >>= 7;
    }
    tmp.reverse();
    tmp
}

#[derive(Clone, Debug)]
pub struct Reader<'a> {
    pub d: &'a [u8],
    pub p: usize,
}

pub type R<T> = Result<T, &'static str>;

impl<'a> Reader<'a> {
    pub fn new(d: &'a [u8]) -> Reader<'a> {
        Reader { d, p: 0 }
    }
    pub fn eof(&self) -> bool {
        self.p >= self.d.len()
    }
    pub fn peek(&self) -> R<u8> {
        self.d.get(self.p).copied().ok_or("eof")
    }
    pub fn byte(&mut self) -> R<u8> {
        let b = self.peek()?;
        self.p += 1;
        Ok(b)
    }
    pub fn take(&mut self, n: usize) -> R<&'a [u8]> {
        let s = self.d.get(self.p..self.p + n).ok_or("truncated")?;
        self.p += n;
        Ok(s)
    }
    pub fn rest(&mut self) -> &'a [u8] {
        let s = &self.d[self.p.min(self.d.len())..];
        self.p = self.d.len();
        s
    }
    pub fn uintvar(&mut self) -> R<u32> {
        let mut v: u32 = 0;
        for _ in 0..5 {
            let b = self.byte()?;
            v = (v << 7) | (b & 0x7F) as u32;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("uintvar")
    }
    pub fn text(&mut self) -> R<String> {
        let mut start = self.p;
        if self.peek()? == 0x7F || self.peek()? == b'"' {
            start += 1;
        }
        let end = start + self.d.get(start..).ok_or("eof")?.iter().position(|&b| b == 0).ok_or("unterminated text")?;
        let s = String::from_utf8_lossy(&self.d[start..end]).into_owned();
        self.p = end + 1;
        Ok(s)
    }
    pub fn value_length(&mut self) -> R<usize> {
        let b = self.byte()?;
        match b {
            0..=30 => Ok(b as usize),
            31 => Ok(self.uintvar()? as usize),
            _ => Err("value-length"),
        }
    }
    pub fn long_int(&mut self) -> R<u64> {
        let n = self.byte()? as usize;
        if n > 8 {
            return Err("long-int");
        }
        Ok(self.take(n)?.iter().fold(0u64, |a, &b| a << 8 | b as u64))
    }
    pub fn integer(&mut self) -> R<u64> {
        let b = self.peek()?;
        if b & 0x80 != 0 {
            self.p += 1;
            Ok((b & 0x7F) as u64)
        } else {
            self.long_int()
        }
    }
    /// Returns the raw bytes of one generic value and advances past it.
    pub fn skip_value(&mut self) -> R<&'a [u8]> {
        let start = self.p;
        let b = self.peek()?;
        match b {
            0..=30 => {
                self.p += 1;
                self.take(b as usize)?;
            }
            31 => {
                self.p += 1;
                let n = self.uintvar()? as usize;
                self.take(n)?;
            }
            32..=127 => {
                self.text()?;
            }
            _ => self.p += 1,
        }
        Ok(&self.d[start..self.p])
    }
    /// Encoded-string-value: Text-string | Value-length Char-set Text-string
    pub fn encoded_string(&mut self) -> R<String> {
        let b = self.peek()?;
        if b <= 31 {
            let len = self.value_length()?;
            let end = self.p + len;
            let mut sub = Reader { d: &self.d[..end.min(self.d.len())], p: self.p };
            let _charset = sub.integer()?;
            let s = sub.text()?;
            self.p = end;
            Ok(s)
        } else {
            self.text()
        }
    }
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn uintvar_rt() {
        for v in [0u32, 1, 127, 128, 300, 16383, 16384, 0xFFFF_FFF] {
            let e = uintvar(v);
            assert_eq!(Reader::new(&e).uintvar().unwrap(), v);
        }
        assert_eq!(uintvar(0x87A5), vec![0x82, 0x8F, 0x25]);
    }
}
