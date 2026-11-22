//! Persistent storage: a versioned binary snapshot of the user's data
//! (contacts, conversations, call log, settings) written to a block device
//! in two alternating, CRC-protected slots so a crash or power cut mid-write
//! never loses the previous good copy.
use crate::mms::Part;
use crate::prelude::*;
use crate::services::{Contact, Core, LogEntry, LogKind, Message, NetMode, Status, Thread, Transport};

pub const SECTOR: usize = 512;
const MAGIC: &[u8; 4] = b"TQDB";
const VERSION: u16 = 1;
const SLOT_MAGIC: &[u8; 8] = b"TQSLOT01";

// ---------------------------------------------------------------- codec

#[derive(Default)]
pub struct Enc {
    pub buf: Vec<u8>,
}

impl Enc {
    pub fn u(&mut self, mut v: u64) {
        loop {
            let b = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(b);
                return;
            }
            self.buf.push(b | 0x80);
        }
    }
    pub fn i(&mut self, v: i64) {
        self.u(((v << 1) ^ (v >> 63)) as u64);
    }
    pub fn b(&mut self, v: bool) {
        self.buf.push(v as u8);
    }
    pub fn bytes(&mut self, d: &[u8]) {
        self.u(d.len() as u64);
        self.buf.extend_from_slice(d);
    }
    pub fn s(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
}

pub struct Dec<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Dec<'a> {
    pub fn new(d: &'a [u8]) -> Dec<'a> {
        Dec { d, p: 0 }
    }
    pub fn u(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = *self.d.get(self.p)?;
            self.p += 1;
            v |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    pub fn n(&mut self) -> Option<usize> {
        let v = self.u()? as usize;
        (v <= self.d.len()).then_some(v)
    }
    pub fn i(&mut self) -> Option<i64> {
        let v = self.u()?;
        Some(((v >> 1) as i64) ^ -((v & 1) as i64))
    }
    pub fn b(&mut self) -> Option<bool> {
        let v = *self.d.get(self.p)?;
        self.p += 1;
        Some(v != 0)
    }
    pub fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.n()?;
        let s = self.d.get(self.p..self.p + n)?.to_vec();
        self.p += n;
        Some(s)
    }
    pub fn s(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
}

fn transport_code(t: Transport) -> u64 {
    match t {
        Transport::Sms => 0,
        Transport::Mms => 1,
        Transport::Rcs => 2,
    }
}

fn status_code(s: Status) -> u64 {
    match s {
        Status::Sending => 0,
        Status::Failed => 1,
        Status::Sent => 2,
        Status::Delivered => 3,
        Status::Read => 4,
        Status::Received => 5,
    }
}

fn status_from(c: u64) -> Status {
    match c {
        // a message still in flight at shutdown didn't make it out
        0 | 1 => Status::Failed,
        2 => Status::Sent,
        3 => Status::Delivered,
        4 => Status::Read,
        _ => Status::Received,
    }
}

impl Core {
    /// Serialize everything the user would expect to survive a reboot.
    pub fn snapshot(&self) -> Vec<u8> {
        let mut e = Enc::default();
        e.buf.extend_from_slice(MAGIC);
        e.buf.extend_from_slice(&VERSION.to_le_bytes());
        let st = &self.settings;
        for b in [st.rcs, st.delivery_reports, st.read_receipts, st.typing_indicators, st.airplane, st.alarm_on, st.ringer] {
            e.b(b);
        }
        e.u(match st.net_mode {
            NetMode::Auto5g => 0,
            NetMode::Lte => 1,
            NetMode::Nr => 2,
        });
        let (h, m) = st.alarm.unwrap_or((7, 0));
        e.u(h as u64);
        e.u(m as u64);
        e.u(self.contacts.len() as u64);
        for c in &self.contacts {
            e.s(&c.name);
            e.s(&c.number);
        }
        e.u(self.log.len() as u64);
        for l in &self.log {
            e.s(&l.number);
            e.u(match l.kind {
                LogKind::Dialed => 0,
                LogKind::Received => 1,
                LogKind::Missed => 2,
            });
            e.i(l.time);
            e.u(l.secs);
        }
        e.u(self.missed as u64);
        e.u(self.next_msg_id() as u64);
        e.u(self.threads.len() as u64);
        for t in &self.threads {
            e.s(&t.number);
            e.s(&t.group_name);
            e.u(t.members.len() as u64);
            for m in &t.members {
                e.s(m);
            }
            e.u(t.msgs.len() as u64);
            for m in &t.msgs {
                e.u(m.id as u64);
                e.b(m.incoming);
                e.u(transport_code(m.transport));
                e.s(&m.text);
                e.b(m.subject.is_some());
                e.s(m.subject.as_deref().unwrap_or(""));
                e.u(m.attachments.len() as u64);
                for a in &m.attachments {
                    e.s(&a.content_type);
                    e.s(&a.name);
                    e.bytes(&a.data);
                }
                e.i(m.time);
                e.u(status_code(m.status));
                e.b(m.unread);
                e.s(&m.imdn_id);
                e.b(m.wants_display);
                e.s(&m.from);
            }
        }
        e.u(self.settings.language.code() as u64);
        e.buf
    }

    /// Restore a snapshot. Returns false (and changes nothing) if it is invalid.
    pub fn restore(&mut self, data: &[u8]) -> bool {
        match self.try_restore(data) {
            Some(()) => true,
            None => false,
        }
    }

    fn try_restore(&mut self, data: &[u8]) -> Option<()> {
        if data.get(0..4)? != MAGIC || u16::from_le_bytes([*data.get(4)?, *data.get(5)?]) != VERSION {
            return None;
        }
        let mut d = Dec::new(&data[6..]);
        let mut st = self.settings.clone();
        st.rcs = d.b()?;
        st.delivery_reports = d.b()?;
        st.read_receipts = d.b()?;
        st.typing_indicators = d.b()?;
        st.airplane = d.b()?;
        st.alarm_on = d.b()?;
        st.ringer = d.b()?;
        st.net_mode = match d.u()? {
            1 => NetMode::Lte,
            2 => NetMode::Nr,
            _ => NetMode::Auto5g,
        };
        st.alarm = Some((d.u()? as u32, d.u()? as u32));
        let mut contacts = Vec::new();
        for _ in 0..d.n()? {
            contacts.push(Contact { name: d.s()?, number: d.s()? });
        }
        let mut log = Vec::new();
        for _ in 0..d.n()? {
            let number = d.s()?;
            let kind = match d.u()? {
                0 => LogKind::Dialed,
                1 => LogKind::Received,
                _ => LogKind::Missed,
            };
            log.push(LogEntry { number, kind, time: d.i()?, secs: d.u()? });
        }
        let missed = d.u()? as usize;
        let next = d.u()? as u32;
        let mut threads = Vec::new();
        for _ in 0..d.n()? {
            let number = d.s()?;
            let group_name = d.s()?;
            let mut members = Vec::new();
            for _ in 0..d.n()? {
                members.push(d.s()?);
            }
            let mut msgs = Vec::new();
            for _ in 0..d.n()? {
                let id = d.u()? as u32;
                let incoming = d.b()?;
                let transport = match d.u()? {
                    1 => Transport::Mms,
                    2 => Transport::Rcs,
                    _ => Transport::Sms,
                };
                let text = d.s()?;
                let has_subject = d.b()?;
                let subject = d.s()?;
                let mut attachments = Vec::new();
                for _ in 0..d.n()? {
                    attachments.push(Part { content_type: d.s()?, name: d.s()?, data: d.bytes()? });
                }
                let time = d.i()?;
                let status = status_from(d.u()?);
                msgs.push(Message {
                    id,
                    incoming,
                    transport,
                    text,
                    subject: has_subject.then_some(subject),
                    attachments,
                    time,
                    status,
                    unread: d.b()?,
                    imdn_id: d.s()?,
                    wants_display: d.b()?,
                    sms_mrs: Vec::new(),
                    from: d.s()?,
                });
            }
            threads.push(Thread { number, msgs, typing_until: 0, members, group_name, typing_who: String::new() });
        }
        // added later: snapshots from older versions end here
        st.language = d.u().map(crate::ui::i18n::Lang::from_code).unwrap_or_default();
        self.settings = st;
        self.contacts = contacts;
        self.log = log;
        self.missed = missed;
        self.threads = threads;
        self.set_next_msg_id(next);
        Some(())
    }
}

// ---------------------------------------------------------------- block storage

pub trait BlockDev {
    fn sectors(&self) -> u64;
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> bool;
    fn write(&mut self, lba: u64, buf: &[u8]) -> bool;
}

pub trait Storage {
    fn load(&mut self) -> Option<Vec<u8>>;
    fn save(&mut self, data: &[u8]) -> bool;
}

pub fn crc32(d: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

/// Two-slot snapshot store. Each slot: header sector (magic, sequence,
/// length, CRC) followed by the data sectors. Saves alternate slots and
/// write the header last; load picks the valid slot with the highest
/// sequence number.
pub struct BlockStore<D: BlockDev> {
    dev: D,
    seq: u64,
    next_slot: usize,
}

impl<D: BlockDev> BlockStore<D> {
    pub fn new(dev: D) -> BlockStore<D> {
        BlockStore { dev, seq: 0, next_slot: 0 }
    }

    fn slot_base(&self, slot: usize) -> u64 {
        slot as u64 * (self.dev.sectors() / 2)
    }

    pub fn capacity(&self) -> usize {
        ((self.dev.sectors() / 2).saturating_sub(1) as usize) * SECTOR
    }

    fn read_slot(&mut self, slot: usize) -> Option<(u64, Vec<u8>)> {
        let base = self.slot_base(slot);
        let mut hdr = [0u8; SECTOR];
        if !self.dev.read(base, &mut hdr) || &hdr[0..8] != SLOT_MAGIC {
            return None;
        }
        let seq = u64::from_le_bytes(hdr[8..16].try_into().ok()?);
        let len = u32::from_le_bytes(hdr[16..20].try_into().ok()?) as usize;
        let crc = u32::from_le_bytes(hdr[20..24].try_into().ok()?);
        if len > self.capacity() {
            return None;
        }
        let mut data = vec![0u8; len.div_ceil(SECTOR) * SECTOR];
        for (i, chunk) in data.chunks_mut(SECTOR).enumerate() {
            if !self.dev.read(base + 1 + i as u64, chunk) {
                return None;
            }
        }
        data.truncate(len);
        (crc32(&data) == crc).then_some((seq, data))
    }

    pub fn into_inner(self) -> D {
        self.dev
    }
}

impl<D: BlockDev> Storage for BlockStore<D> {
    fn load(&mut self) -> Option<Vec<u8>> {
        let a = self.read_slot(0);
        let b = self.read_slot(1);
        let (slot, (seq, data)) = match (a, b) {
            (Some(a), Some(b)) => {
                if a.0 >= b.0 {
                    (0, a)
                } else {
                    (1, b)
                }
            }
            (Some(a), None) => (0, a),
            (None, Some(b)) => (1, b),
            (None, None) => return None,
        };
        self.seq = seq;
        self.next_slot = 1 - slot;
        Some(data)
    }

    fn save(&mut self, data: &[u8]) -> bool {
        if data.len() > self.capacity() || self.dev.sectors() < 4 {
            return false;
        }
        let slot = self.next_slot;
        let base = self.slot_base(slot);
        for (i, chunk) in data.chunks(SECTOR).enumerate() {
            let mut s = [0u8; SECTOR];
            s[..chunk.len()].copy_from_slice(chunk);
            if !self.dev.write(base + 1 + i as u64, &s) {
                return false;
            }
        }
        self.seq += 1;
        let mut hdr = [0u8; SECTOR];
        hdr[0..8].copy_from_slice(SLOT_MAGIC);
        hdr[8..16].copy_from_slice(&self.seq.to_le_bytes());
        hdr[16..20].copy_from_slice(&(data.len() as u32).to_le_bytes());
        hdr[20..24].copy_from_slice(&crc32(data).to_le_bytes());
        if !self.dev.write(base, &hdr) {
            return false;
        }
        self.next_slot = 1 - slot;
        true
    }
}

/// In-memory block device (tests, or a phone with no storage).
pub struct RamDisk(pub Vec<u8>);

impl BlockDev for RamDisk {
    fn sectors(&self) -> u64 {
        (self.0.len() / SECTOR) as u64
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> bool {
        let o = lba as usize * SECTOR;
        match self.0.get(o..o + buf.len()) {
            Some(s) => {
                buf.copy_from_slice(s);
                true
            }
            None => false,
        }
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> bool {
        let o = lba as usize * SECTOR;
        match self.0.get_mut(o..o + buf.len()) {
            Some(s) => {
                s.copy_from_slice(buf);
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn varints_and_crc() {
        let mut e = Enc::default();
        for v in [0i64, -1, 1, i64::MIN, i64::MAX, 1_795_280_400] {
            e.i(v);
        }
        let mut d = Dec::new(&e.buf);
        for v in [0i64, -1, 1, i64::MIN, i64::MAX, 1_795_280_400] {
            assert_eq!(d.i(), Some(v));
        }
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn two_slot_store_survives_torn_write() {
        let mut st = BlockStore::new(RamDisk(vec![0; 64 * SECTOR]));
        assert!(st.load().is_none());
        assert!(st.save(b"first"));
        assert!(st.save(&vec![7u8; 3000]));
        let mut dev = st.into_inner();
        let mut st2 = BlockStore::new(RamDisk(dev.0.clone()));
        assert_eq!(st2.load(), Some(vec![7u8; 3000]));
        // corrupt the newest slot (slot 1): we fall back to the older one
        let o = 33 * SECTOR + 10;
        dev.0[o] ^= 0xFF;
        let mut st3 = BlockStore::new(dev);
        assert_eq!(st3.load().as_deref(), Some(&b"first"[..]));
        // and the next save goes to the damaged slot, not over the good one
        assert!(st3.save(b"third"));
        assert_eq!(st3.load().as_deref(), Some(&b"third"[..]));
    }
}
