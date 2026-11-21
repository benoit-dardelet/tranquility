//! Small helpers: hex, calendar maths, PRNG, text layout, number matching.
use crate::prelude::*;

pub fn hex(data: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let b = s.trim().as_bytes();
    if b.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(b.len() / 2);
    for c in b.chunks(2) {
        out.push((nib(c[0])? << 4) | nib(c[1])?);
    }
    Some(out)
}

fn nib(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------- calendar

/// Days since 1970-01-01 for a proleptic Gregorian date.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn unix_from(y: i64, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> i64 {
    days_from_civil(y, mo, d) * 86400 + (h * 3600 + mi * 60 + s) as i64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    /// 0 = Sunday
    pub wday: u32,
}

pub fn civil_from_unix(t: i64) -> DateTime {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    DateTime {
        year: if month <= 2 { y + 1 } else { y },
        month,
        day,
        hour: (secs / 3600) as u32,
        min: (secs % 3600 / 60) as u32,
        sec: (secs % 60) as u32,
        wday: (days + 4).rem_euclid(7) as u32,
    }
}

pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
pub const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub fn iso8601(t: i64) -> String {
    let d = civil_from_unix(t);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        d.year, d.month, d.day, d.hour, d.min, d.sec
    )
}

pub fn hhmm(t: i64) -> String {
    let d = civil_from_unix(t);
    format!("{:02}:{:02}", d.hour, d.min)
}

pub fn duration(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
    } else {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    }
}

// ---------------------------------------------------------------- PRNG

/// xorshift64* — good enough for tags, branches and message ids.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }
    pub fn token(&mut self, len: usize) -> String {
        const A: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        (0..len).map(|_| A[self.below(A.len() as u64) as usize] as char).collect()
    }
}

// ---------------------------------------------------------------- text

/// Terminal display width of a character (East Asian wide / emoji = 2).
pub fn char_width(c: char) -> usize {
    let u = c as u32;
    let wide = matches!(u,
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF |
        0xA000..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 |
        0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x1FA70..=0x1FAFF | 0x20000..=0x3FFFD);
    if wide {
        2
    } else if u < 0x20 || (0x300..=0x36F).contains(&u) || u == 0x200D || (0xFE00..=0xFE0F).contains(&u) {
        0
    } else {
        1
    }
}

/// Display width of a string in terminal columns.
pub fn char_len(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

pub fn trunc(s: &str, n: usize) -> String {
    if char_len(s) <= n {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = char_width(c);
        if w + cw + 1 > n {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

pub fn pad(s: &str, n: usize) -> String {
    let mut out = trunc(s, n);
    for _ in char_len(&out)..n {
        out.push(' ');
    }
    out
}

pub fn center(s: &str, n: usize) -> String {
    let s = trunc(s, n);
    let l = char_len(&s);
    let left = (n - l) / 2;
    let mut out = String::new();
    for _ in 0..left {
        out.push(' ');
    }
    out.push_str(&s);
    pad(&out, n)
}

/// Split a string into a head of at most `n` columns and the rest.
fn split_width(s: &str, n: usize) -> (String, String) {
    let mut head = String::new();
    let mut w = 0;
    let mut it = s.chars().peekable();
    while let Some(&c) = it.peek() {
        let cw = char_width(c);
        if w + cw > n && w > 0 {
            break;
        }
        head.push(c);
        w += cw;
        it.next();
    }
    (head, it.collect())
}

/// Word-wrap to `width` columns, breaking over-long words.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let mut word: String = word.to_string();
            loop {
                let wl = char_len(&word);
                let ll = char_len(&line);
                if ll == 0 && wl <= width {
                    line = word;
                    break;
                } else if ll > 0 && ll + 1 + wl <= width {
                    line.push(' ');
                    line.push_str(&word);
                    break;
                } else if ll > 0 {
                    out.push(core::mem::take(&mut line));
                } else {
                    // word longer than the line
                    let (head, tail) = split_width(&word, width);
                    out.push(head);
                    word = tail;
                }
            }
        }
        out.push(line);
    }
    out
}

pub fn eq_ic(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub fn starts_with_ic(s: &str, p: &str) -> bool {
    s.len() >= p.len() && s.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes())
}

/// Split an AT argument list on commas, honouring double quotes, and strip quotes.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    for c in s.chars() {
        match c {
            '"' => q = !q,
            ',' if !q => out.push(core::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

pub fn normalize_number(n: &str) -> String {
    n.chars()
        .filter(|c| c.is_ascii_digit() || matches!(c, '+' | '*' | '#'))
        .collect()
}

/// Loose phone-number equality (compares the last 10 digits).
pub fn same_number(a: &str, b: &str) -> bool {
    if a.starts_with("group:") || b.starts_with("group:") {
        return a == b;
    }
    let da: Vec<char> = a.chars().filter(|c| c.is_ascii_digit()).collect();
    let db: Vec<char> = b.chars().filter(|c| c.is_ascii_digit()).collect();
    if da.is_empty() || db.is_empty() {
        return a == b;
    }
    let n = da.len().min(db.len()).min(10);
    da[da.len() - n..] == db[db.len() - n..]
}

/// Very small XML helper: text content of the first `<tag>…</tag>` (ignores namespaces prefixes).
pub fn xml_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}", tag);
    let start = xml.find(&open)?;
    let gt = start + xml[start..].find('>')?;
    if xml[..gt].ends_with('/') {
        return Some("");
    }
    let close = format!("</{}>", tag);
    let end = gt + xml[gt..].find(&close)?;
    Some(&xml[gt + 1..end])
}
