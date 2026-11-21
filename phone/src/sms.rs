//! 3GPP TS 23.040 SMS transfer-layer PDUs (SMS-SUBMIT, SMS-DELIVER,
//! SMS-STATUS-REPORT), user-data headers (concatenation, application port
//! addressing), segmentation and reassembly.
use crate::gsm7;
use crate::prelude::*;
use crate::util;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coding {
    Gsm7,
    Bit8,
    Ucs2,
}

impl Coding {
    pub fn dcs(self) -> u8 {
        match self {
            Coding::Gsm7 => 0x00,
            Coding::Bit8 => 0x04,
            Coding::Ucs2 => 0x08,
        }
    }
    /// TS 23.038 §4 data coding scheme interpretation.
    pub fn from_dcs(dcs: u8) -> Coding {
        match dcs >> 4 {
            0x0..=0x3 | 0x4..=0x7 => match (dcs >> 2) & 3 {
                1 => Coding::Bit8,
                2 => Coding::Ucs2,
                _ => Coding::Gsm7,
            },
            0xE => Coding::Ucs2,
            0xF => {
                if dcs & 0x04 != 0 {
                    Coding::Bit8
                } else {
                    Coding::Gsm7
                }
            }
            _ => Coding::Gsm7,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Concat {
    pub reference: u16,
    pub total: u8,
    pub seq: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Udh {
    pub concat: Option<Concat>,
    /// (destination port, source port) — 16-bit application port addressing
    pub ports: Option<(u16, u16)>,
}

impl Udh {
    pub fn is_empty(&self) -> bool {
        self.concat.is_none() && self.ports.is_none()
    }

    /// Encoded header including the leading UDHL octet (empty if no IEs).
    pub fn encode(&self) -> Vec<u8> {
        let mut ies = Vec::new();
        if let Some(c) = self.concat {
            if c.reference > 0xFF {
                ies.extend_from_slice(&[0x08, 4, (c.reference >> 8) as u8, c.reference as u8, c.total, c.seq]);
            } else {
                ies.extend_from_slice(&[0x00, 3, c.reference as u8, c.total, c.seq]);
            }
        }
        if let Some((d, s)) = self.ports {
            ies.extend_from_slice(&[0x05, 4, (d >> 8) as u8, d as u8, (s >> 8) as u8, s as u8]);
        }
        if ies.is_empty() {
            return ies;
        }
        let mut out = vec![ies.len() as u8];
        out.extend(ies);
        out
    }

    /// Decode the information elements (without the UDHL octet).
    pub fn decode(d: &[u8]) -> Udh {
        let mut u = Udh::default();
        let mut p = 0;
        while p + 2 <= d.len() {
            let (iei, len) = (d[p], d[p + 1] as usize);
            let v = &d[(p + 2).min(d.len())..(p + 2 + len).min(d.len())];
            match (iei, v.len()) {
                (0x00, 3) => u.concat = Some(Concat { reference: v[0] as u16, total: v[1], seq: v[2] }),
                (0x08, 4) => {
                    u.concat = Some(Concat { reference: (v[0] as u16) << 8 | v[1] as u16, total: v[2], seq: v[3] })
                }
                (0x05, 4) => u.ports = Some(((v[0] as u16) << 8 | v[1] as u16, (v[2] as u16) << 8 | v[3] as u16)),
                (0x04, 2) => u.ports = Some((v[0] as u16, v[1] as u16)),
                _ => {}
            }
            p += 2 + len;
        }
        u
    }
}

/// Encode TP-UD; returns (TP-UDL, TP-UD).
pub fn encode_ud(coding: Coding, udh: &Udh, text: &str, data: &[u8]) -> Option<(u8, Vec<u8>)> {
    let h = udh.encode();
    match coding {
        Coding::Gsm7 => {
            let sept = gsm7::encode(text)?;
            let hbits = h.len() * 8;
            let off = hbits + (7 - hbits % 7) % 7;
            let mut out = gsm7::pack(&sept, off);
            if out.len() < h.len() {
                out.resize(h.len(), 0);
            }
            out[..h.len()].copy_from_slice(&h);
            Some(((off / 7 + sept.len()) as u8, out))
        }
        Coding::Ucs2 => {
            let mut out = h;
            for u in text.encode_utf16() {
                out.push((u >> 8) as u8);
                out.push(u as u8);
            }
            Some((out.len() as u8, out))
        }
        Coding::Bit8 => {
            let mut out = h;
            out.extend_from_slice(data);
            Some((out.len() as u8, out))
        }
    }
}

/// Decode TP-UD into (header, text, raw payload bytes).
pub fn decode_ud(coding: Coding, udhi: bool, udl: u8, ud: &[u8]) -> (Udh, String, Vec<u8>) {
    let (udh, hlen) = if udhi && !ud.is_empty() {
        let l = ud[0] as usize;
        (Udh::decode(&ud[1..(1 + l).min(ud.len())]), l + 1)
    } else {
        (Udh::default(), 0)
    };
    match coding {
        Coding::Gsm7 => {
            let hbits = hlen * 8;
            let off = hbits + (7 - hbits % 7) % 7;
            let n = (udl as usize).saturating_sub(off / 7);
            let sept = gsm7::unpack(ud, off, n);
            (udh, gsm7::decode(&sept), Vec::new())
        }
        Coding::Ucs2 => {
            let body = &ud[hlen.min(ud.len())..(udl as usize).min(ud.len())];
            let units: Vec<u16> = body.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0] as u16) << 8 | c[1] as u16).collect();
            (udh, String::from_utf16_lossy(&units), Vec::new())
        }
        Coding::Bit8 => {
            let body = ud[hlen.min(ud.len())..(udl as usize).min(ud.len())].to_vec();
            (udh, String::from_utf8_lossy(&body).into_owned(), body)
        }
    }
}

// ---------------------------------------------------------------- addresses

fn bcd_digit(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'*' => 0xA,
        b'#' => 0xB,
        _ => 0xF,
    }
}

fn digit_char(n: u8) -> Option<char> {
    match n {
        0..=9 => Some((b'0' + n) as char),
        0xA => Some('*'),
        0xB => Some('#'),
        _ => None,
    }
}

fn bcd(digits: &[u8]) -> Vec<u8> {
    digits
        .chunks(2)
        .map(|p| bcd_digit(p[0]) | (if p.len() > 1 { bcd_digit(p[1]) } else { 0xF }) << 4)
        .collect()
}

/// TP-OA / TP-DA / TP-RA: digit count, type of address, swapped BCD.
pub fn encode_address(num: &str) -> Vec<u8> {
    let digits: Vec<u8> = num.bytes().filter(|c| c.is_ascii_digit() || *c == b'*' || *c == b'#').collect();
    let mut out = vec![digits.len() as u8, if num.starts_with('+') { 0x91 } else { 0x81 }];
    out.extend(bcd(&digits));
    out
}

/// RP-SMSC address: length in octets (incl. TOA). `None` → "use default SMSC" (00).
pub fn encode_smsc(num: Option<&str>) -> Vec<u8> {
    match num {
        None => vec![0],
        Some(n) => {
            let a = encode_address(n);
            let mut out = vec![(a.len() - 1) as u8];
            out.extend_from_slice(&a[1..]);
            out
        }
    }
}

fn decode_bcd(b: &[u8], ndigits: usize) -> String {
    let mut s = String::new();
    for &x in b {
        for n in [x & 0xF, x >> 4] {
            if s.len() < ndigits {
                if let Some(c) = digit_char(n) {
                    s.push(c);
                }
            }
        }
    }
    s
}

pub fn decode_address(d: &[u8], p: &mut usize) -> Option<String> {
    let len = *d.get(*p)? as usize;
    let toa = *d.get(*p + 1)?;
    let nbytes = len.div_ceil(2);
    let body = d.get(*p + 2..*p + 2 + nbytes)?;
    *p += 2 + nbytes;
    if toa & 0x70 == 0x50 {
        let septets = len * 4 / 7;
        return Some(gsm7::decode(&gsm7::unpack(body, 0, septets)));
    }
    let mut s = String::new();
    if toa & 0x70 == 0x10 {
        s.push('+');
    }
    s.push_str(&decode_bcd(body, len));
    Some(s)
}

// ---------------------------------------------------------------- timestamps

fn swap_bcd(v: u32) -> u8 {
    (((v % 10) << 4) | (v / 10 % 10)) as u8
}

fn unswap(b: u8) -> u32 {
    ((b & 0xF) * 10 + (b >> 4)) as u32
}

/// TP-SCTS for a UTC unix time with a timezone in quarter hours.
pub fn encode_scts(t: i64, tz_q: i8) -> [u8; 7] {
    let d = util::civil_from_unix(t + tz_q as i64 * 900);
    let q = tz_q.unsigned_abs() as u32;
    let mut tz = swap_bcd(q);
    if tz_q < 0 {
        tz |= 0x08;
    }
    [
        swap_bcd((d.year % 100) as u32),
        swap_bcd(d.month),
        swap_bcd(d.day),
        swap_bcd(d.hour),
        swap_bcd(d.min),
        swap_bcd(d.sec),
        tz,
    ]
}

pub fn decode_scts(b: &[u8]) -> i64 {
    if b.len() < 7 {
        return 0;
    }
    let local = util::unix_from(2000 + unswap(b[0]) as i64, unswap(b[1]), unswap(b[2]), unswap(b[3]), unswap(b[4]), unswap(b[5]));
    let q = ((b[6] & 0x07) * 10 + (b[6] >> 4)) as i64;
    let q = if b[6] & 0x08 != 0 { -q } else { q };
    local - q * 900
}

// ---------------------------------------------------------------- TPDUs

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submit {
    pub to: String,
    pub mr: u8,
    pub srr: bool,
    pub udh: Udh,
    pub coding: Coding,
    pub text: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deliver {
    pub from: String,
    pub scts: i64,
    pub udh: Udh,
    pub coding: Coding,
    pub text: String,
    pub data: Vec<u8>,
    pub status_report_indication: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusReport {
    pub mr: u8,
    pub recipient: String,
    pub scts: i64,
    pub discharge: i64,
    /// TP-ST: 0x00..0x1F = transaction completed (0 = delivered)
    pub status: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tpdu {
    Submit(Submit),
    Deliver(Deliver),
    Status(StatusReport),
}

pub fn encode_submit(s: &Submit) -> Option<Vec<u8>> {
    let mut fo = 0x01 | 0x10; // SMS-SUBMIT, relative validity period
    if s.srr {
        fo |= 0x20;
    }
    if !s.udh.is_empty() {
        fo |= 0x40;
    }
    let (udl, ud) = encode_ud(s.coding, &s.udh, &s.text, &s.data)?;
    let mut out = vec![fo, s.mr];
    out.extend(encode_address(&s.to));
    out.extend_from_slice(&[0x00, s.coding.dcs(), 0xAA, udl]);
    out.extend(ud);
    Some(out)
}

pub fn encode_deliver(d: &Deliver, tz_q: i8) -> Option<Vec<u8>> {
    let mut fo = 0x04; // SMS-DELIVER, TP-MMS: no more messages
    if d.status_report_indication {
        fo |= 0x20;
    }
    if !d.udh.is_empty() {
        fo |= 0x40;
    }
    let (udl, ud) = encode_ud(d.coding, &d.udh, &d.text, &d.data)?;
    let mut out = vec![fo];
    out.extend(encode_address(&d.from));
    out.extend_from_slice(&[0x00, d.coding.dcs()]);
    out.extend_from_slice(&encode_scts(d.scts, tz_q));
    out.push(udl);
    out.extend(ud);
    Some(out)
}

pub fn encode_status_report(r: &StatusReport, tz_q: i8) -> Vec<u8> {
    let mut out = vec![0x02 | 0x20, r.mr];
    out.extend(encode_address(&r.recipient));
    out.extend_from_slice(&encode_scts(r.scts, tz_q));
    out.extend_from_slice(&encode_scts(r.discharge, tz_q));
    out.push(r.status);
    out
}

/// Decode a PDU as exchanged with the modem in PDU mode (leading SMSC address).
pub fn decode(pdu: &[u8]) -> Option<Tpdu> {
    let sc_len = *pdu.first()? as usize;
    decode_tpdu(pdu.get(1 + sc_len..)?)
}

pub fn decode_tpdu(d: &[u8]) -> Option<Tpdu> {
    let fo = *d.first()?;
    let mut p = 1;
    match fo & 0x03 {
        0 => {
            let from = decode_address(d, &mut p)?;
            let _pid = *d.get(p)?;
            let coding = Coding::from_dcs(*d.get(p + 1)?);
            let scts = decode_scts(d.get(p + 2..p + 9)?);
            let udl = *d.get(p + 9)?;
            let (udh, text, data) = decode_ud(coding, fo & 0x40 != 0, udl, d.get(p + 10..)?);
            Some(Tpdu::Deliver(Deliver { from, scts, udh, coding, text, data, status_report_indication: fo & 0x20 != 0 }))
        }
        1 => {
            let mr = *d.get(1)?;
            p = 2;
            let to = decode_address(d, &mut p)?;
            let coding = Coding::from_dcs(*d.get(p + 1)?);
            p += 2;
            p += match (fo >> 3) & 3 {
                0 => 0,
                2 => 1,
                _ => 7,
            };
            let udl = *d.get(p)?;
            let (udh, text, data) = decode_ud(coding, fo & 0x40 != 0, udl, d.get(p + 1..)?);
            Some(Tpdu::Submit(Submit { to, mr, srr: fo & 0x20 != 0, udh, coding, text, data }))
        }
        2 => {
            let mr = *d.get(1)?;
            p = 2;
            let recipient = decode_address(d, &mut p)?;
            let scts = decode_scts(d.get(p..p + 7)?);
            let discharge = decode_scts(d.get(p + 7..p + 14)?);
            let status = *d.get(p + 14)?;
            Some(Tpdu::Status(StatusReport { mr, recipient, scts, discharge, status }))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- segmentation

/// Split text into segments that each fit one SMS. Returns the coding used.
pub fn segment_text(text: &str) -> (Coding, Vec<String>) {
    if let Some(sept) = gsm7::encode(text) {
        if sept.len() <= 160 {
            return (Coding::Gsm7, vec![text.to_string()]);
        }
        let mut segs = Vec::new();
        let mut cur = String::new();
        let mut cur_len = 0;
        for c in text.chars() {
            let l = gsm7::encode(c.encode_utf8(&mut [0; 4])).map(|v| v.len()).unwrap_or(1);
            if cur_len + l > 153 {
                segs.push(core::mem::take(&mut cur));
                cur_len = 0;
            }
            cur.push(c);
            cur_len += l;
        }
        if !cur.is_empty() {
            segs.push(cur);
        }
        return (Coding::Gsm7, segs);
    }
    let units = text.encode_utf16().count();
    if units <= 70 {
        return (Coding::Ucs2, vec![text.to_string()]);
    }
    let mut segs = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0;
    for c in text.chars() {
        let l = c.len_utf16();
        if cur_len + l > 67 {
            segs.push(core::mem::take(&mut cur));
            cur_len = 0;
        }
        cur.push(c);
        cur_len += l;
    }
    if !cur.is_empty() {
        segs.push(cur);
    }
    (Coding::Ucs2, segs)
}

/// Build the SMS-SUBMIT TPDUs for a (possibly long) text.
pub fn build_submits(to: &str, text: &str, srr: bool, reference: u8) -> Vec<Vec<u8>> {
    let (coding, segs) = segment_text(text);
    let total = segs.len();
    segs.iter()
        .enumerate()
        .filter_map(|(i, seg)| {
            let udh = if total > 1 {
                Udh { concat: Some(Concat { reference: reference as u16, total: total as u8, seq: i as u8 + 1 }), ports: None }
            } else {
                Udh::default()
            };
            encode_submit(&Submit { to: to.to_string(), mr: 0, srr, udh, coding, text: seg.clone(), data: Vec::new() })
        })
        .collect()
}

/// Build SMS-DELIVER TPDUs carrying binary data to an application port (e.g. WAP push).
pub fn build_port_delivers(from: &str, data: &[u8], ports: (u16, u16), reference: u8, scts: i64, tz_q: i8) -> Vec<Vec<u8>> {
    let chunks: Vec<&[u8]> = data.chunks(128).collect();
    let total = chunks.len();
    chunks
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let concat = (total > 1).then_some(Concat { reference: reference as u16, total: total as u8, seq: i as u8 + 1 });
            let d = Deliver {
                from: from.to_string(),
                scts,
                udh: Udh { concat, ports: Some(ports) },
                coding: Coding::Bit8,
                text: String::new(),
                data: c.to_vec(),
                status_report_indication: false,
            };
            encode_deliver(&d, tz_q)
        })
        .collect()
}

pub fn build_text_delivers(from: &str, text: &str, reference: u8, scts: i64, tz_q: i8) -> Vec<Vec<u8>> {
    let (coding, segs) = segment_text(text);
    let total = segs.len();
    segs.iter()
        .enumerate()
        .filter_map(|(i, seg)| {
            let concat = (total > 1).then_some(Concat { reference: reference as u16, total: total as u8, seq: i as u8 + 1 });
            let d = Deliver {
                from: from.to_string(),
                scts,
                udh: Udh { concat, ports: None },
                coding,
                text: seg.clone(),
                data: Vec::new(),
                status_report_indication: false,
            };
            encode_deliver(&d, tz_q)
        })
        .collect()
}

/// Reassembles concatenated SMS-DELIVER segments.
#[derive(Default)]
pub struct Reassembler {
    parts: BTreeMap<(String, u16), Vec<Option<Deliver>>>,
}

impl Reassembler {
    pub fn push(&mut self, d: Deliver) -> Option<Deliver> {
        let c = match d.udh.concat {
            Some(c) if c.total > 1 => c,
            _ => return Some(d),
        };
        let key = (d.from.clone(), c.reference);
        let slots = self.parts.entry(key.clone()).or_insert_with(|| vec![None; c.total as usize]);
        let idx = (c.seq as usize).saturating_sub(1);
        if idx < slots.len() {
            slots[idx] = Some(d);
        }
        if slots.iter().any(|s| s.is_none()) {
            return None;
        }
        let slots = self.parts.remove(&key)?;
        let mut it = slots.into_iter().flatten();
        let mut first = it.next()?;
        for s in it {
            first.text.push_str(&s.text);
            first.data.extend(s.data);
        }
        first.udh.concat = None;
        Some(first)
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::util::{hex, unhex};

    #[test]
    fn submit_known_vector() {
        // "hellohello" to +15550101 without SRR
        let s = Submit { to: "+15550101".into(), mr: 0, srr: false, udh: Udh::default(), coding: Coding::Gsm7, text: "hellohello".into(), data: vec![] };
        let pdu = encode_submit(&s).unwrap();
        assert_eq!(hex(&pdu), "11000891515510100000AA0AE8329BFD4697D9EC37");
        let mut full = vec![0];
        full.extend(&pdu);
        assert_eq!(decode(&full), Some(Tpdu::Submit(s)));
    }

    #[test]
    fn deliver_roundtrip_alnum_and_scts() {
        let t = crate::util::unix_from(2026, 11, 21, 17, 0, 0);
        let d = Deliver { from: "+15550102".into(), scts: t, udh: Udh::default(), coding: Coding::Ucs2, text: "Héllo 日本".into(), data: vec![], status_report_indication: false };
        let pdu = encode_deliver(&d, -20).unwrap();
        assert_eq!(decode_tpdu(&pdu), Some(Tpdu::Deliver(d)));
        // Real-world deliver PDU with an alphanumeric originator
        let raw = unhex("0791448720003023240DD0E474D81C0EBB010000111011315214000BE474D81C0EBB5DE3771B").unwrap();
        match decode(&raw).unwrap() {
            Tpdu::Deliver(d) => {
                assert_eq!(d.from, "diafaan");
                assert_eq!(d.text, "diafaan.com");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn long_message_concat() {
        let text: String = core::iter::repeat("0123456789").take(40).collect();
        let pdus = build_text_delivers("+15550103", &text, 7, 0, 0);
        assert_eq!(pdus.len(), 3);
        let mut r = Reassembler::default();
        let mut out = None;
        for p in pdus.iter().rev() {
            if let Tpdu::Deliver(d) = decode_tpdu(p).unwrap() {
                out = r.push(d);
            }
        }
        assert_eq!(out.unwrap().text, text);
        let subs = build_submits("+15550103", &text, true, 9);
        assert_eq!(subs.len(), 3);
    }

    #[test]
    fn status_report() {
        let t = crate::util::unix_from(2026, 11, 21, 17, 0, 0);
        let r = StatusReport { mr: 42, recipient: "+15550101".into(), scts: t, discharge: t + 60, status: 0 };
        assert_eq!(decode_tpdu(&encode_status_report(&r, 0)), Some(Tpdu::Status(r)));
    }
}
