//! AT command driver for the baseband (3GPP TS 27.007 / 27.005 plus the
//! Quectel-style TCP/IP socket extension). The baseband is reached through
//! a byte-oriented [`Link`] — a UART on real hardware, or the in-memory
//! simulated modem.
use crate::prelude::*;
use crate::util::starts_with_ic;

pub trait Link {
    fn write(&mut self, data: &[u8]);
    fn read(&mut self, out: &mut Vec<u8>);
    /// Give the link a chance to make progress (used by the simulator).
    fn poll(&mut self, _now_ms: u64) {}
    /// 8 kHz call downlink audio, if the baseband routes it to the host.
    fn voice(&mut self, _out: &mut [i16]) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Line(String),
    Prompt,
}

#[derive(Default)]
pub struct LineReader {
    buf: Vec<u8>,
}

impl LineReader {
    pub fn push(&mut self, data: &[u8], out: &mut VecDeque<Item>) {
        for &b in data {
            match b {
                b'\r' | b'\n' => {
                    if !self.buf.is_empty() {
                        out.push_back(Item::Line(String::from_utf8_lossy(&self.buf).into_owned()));
                        self.buf.clear();
                    }
                }
                _ => {
                    self.buf.push(b);
                    if self.buf == b"> " {
                        out.push_back(Item::Prompt);
                        self.buf.clear();
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Urc {
    pub line: String,
    /// Second line for PDU-carrying URCs (+CMT, +CDS, +CBM).
    pub pdu: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resp {
    pub ok: bool,
    pub lines: Vec<String>,
    pub error: Option<String>,
}

impl Resp {
    /// Payload of the first line starting with `prefix` (e.g. "+CSQ:").
    pub fn get(&self, prefix: &str) -> Option<&str> {
        self.lines.iter().find(|l| l.starts_with(prefix)).map(|l| l[prefix.len()..].trim())
    }
    pub fn all(&self, prefix: &str) -> Vec<&str> {
        self.lines.iter().filter(|l| l.starts_with(prefix)).map(|l| l[prefix.len()..].trim()).collect()
    }
    pub fn first(&self) -> Option<&str> {
        self.lines.first().map(|s| s.as_str())
    }
}

const URC_PREFIXES: &[&str] = &[
    "RING", "+CRING:", "+CLIP:", "+CMT:", "+CDS:", "+CBM:", "+CMTI:", "+CEREG:", "+C5GREG:", "+CREG:", "+CGREG:", "+QIURC:",
    "+QIOPEN:", "+CUSD:", "NO CARRIER", "BUSY", "NO ANSWER", "+CTZE:", "+CTZV:", "+CIEV:",
];
const PDU_URCS: &[&str] = &["+CMT:", "+CDS:", "+CBM:"];

fn is_urc(l: &str) -> bool {
    URC_PREFIXES.iter().any(|p| l.starts_with(p))
}

/// "AT+CSQ" -> "+CSQ:", "AT+CPIN?" -> "+CPIN:", "ATD123;" -> ""
fn cmd_prefix(c: &str) -> String {
    let body = &c[2.min(c.len())..];
    if !body.starts_with('+') {
        return String::new();
    }
    let end = body.find(['=', '?']).unwrap_or(body.len());
    format!("{}:", &body[..end])
}

pub type Clock = fn() -> u64;

pub struct Modem {
    link: Box<dyn Link>,
    reader: LineReader,
    items: VecDeque<Item>,
    urcs: VecDeque<Urc>,
    expect_pdu: Option<String>,
    clock: Clock,
    /// Rolling trace of AT traffic (for the engineering screen).
    pub trace: VecDeque<String>,
}

impl Modem {
    pub fn new(link: Box<dyn Link>, clock: Clock) -> Modem {
        Modem { link, reader: LineReader::default(), items: VecDeque::new(), urcs: VecDeque::new(), expect_pdu: None, clock, trace: VecDeque::new() }
    }

    fn log(&mut self, dir: &str, s: &str) {
        if self.trace.len() >= 64 {
            self.trace.pop_front();
        }
        let mut t: String = s.chars().take(60).collect();
        t.insert_str(0, dir);
        self.trace.push_back(t);
    }

    fn pump(&mut self) {
        self.link.poll((self.clock)());
        let mut v = Vec::new();
        self.link.read(&mut v);
        if !v.is_empty() {
            self.reader.push(&v, &mut self.items);
        }
    }

    fn unsolicited(&mut self, l: String) {
        if let Some(prefix) = self.expect_pdu.take() {
            self.log("< ", &l);
            self.urcs.push_back(Urc { line: prefix, pdu: Some(l) });
            return;
        }
        if starts_with_ic(&l, "AT") || l == "OK" {
            return; // echo / stray final result
        }
        self.log("< ", &l);
        if PDU_URCS.iter().any(|p| l.starts_with(p)) {
            self.expect_pdu = Some(l);
            return;
        }
        self.urcs.push_back(Urc { line: l, pdu: None });
    }

    /// Read whatever the modem has sent and queue the URCs.
    pub fn poll(&mut self) {
        self.pump();
        while let Some(it) = self.items.pop_front() {
            if let Item::Line(l) = it {
                self.unsolicited(l);
            }
        }
    }

    pub fn voice(&mut self, out: &mut [i16]) -> bool {
        self.link.voice(out)
    }

    pub fn take_urc(&mut self) -> Option<Urc> {
        self.urcs.pop_front()
    }

    pub fn cmd(&mut self, c: &str) -> Resp {
        self.exec(c, None, 5000)
    }

    /// Command that expects a "> " prompt followed by a payload (e.g. AT+CMGS).
    pub fn cmd_payload(&mut self, c: &str, payload: &[u8]) -> Resp {
        self.exec(c, Some(payload), 10000)
    }

    fn exec(&mut self, c: &str, payload: Option<&[u8]>, timeout: u64) -> Resp {
        self.poll();
        let prefix = cmd_prefix(c);
        let dial = starts_with_ic(c, "ATD") || starts_with_ic(c, "ATA");
        self.log("> ", c);
        self.link.write(c.as_bytes());
        self.link.write(b"\r");
        let start = (self.clock)();
        let mut lines = Vec::new();
        let mut spins = 0u32;
        loop {
            self.pump();
            while let Some(it) = self.items.pop_front() {
                let l = match it {
                    Item::Prompt => {
                        if let Some(p) = payload {
                            self.link.write(p);
                        }
                        continue;
                    }
                    Item::Line(l) => l,
                };
                if self.expect_pdu.is_some() {
                    self.unsolicited(l);
                    continue;
                }
                let fin = |ok: bool, err: Option<String>, lines: Vec<String>| Resp { ok, lines, error: err };
                if l == "OK" || l == "SEND OK" {
                    self.log("< ", &l);
                    return fin(true, None, lines);
                }
                if l == "ERROR" || l == "SEND FAIL" || l.starts_with("+CME ERROR") || l.starts_with("+CMS ERROR") {
                    self.log("< ", &l);
                    return fin(false, Some(l), lines);
                }
                if dial && matches!(l.as_str(), "NO CARRIER" | "BUSY" | "NO ANSWER" | "NO DIALTONE") {
                    return fin(false, Some(l), lines);
                }
                if starts_with_ic(&l, "AT") {
                    continue; // command echo
                }
                if !prefix.is_empty() && l.starts_with(prefix.as_str()) {
                    self.log("< ", &l);
                    lines.push(l);
                } else if is_urc(&l) {
                    self.unsolicited(l);
                } else {
                    self.log("< ", &l);
                    lines.push(l);
                }
            }
            spins += 1;
            if (self.clock)().saturating_sub(start) > timeout || spins > 200_000 {
                self.log("! ", "timeout");
                return Resp { ok: false, lines, error: Some("TIMEOUT".into()) };
            }
            core::hint::spin_loop();
        }
    }
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn prefixes() {
        assert_eq!(cmd_prefix("AT+CSQ"), "+CSQ:");
        assert_eq!(cmd_prefix("AT+CPIN?"), "+CPIN:");
        assert_eq!(cmd_prefix("AT+CMGS=23"), "+CMGS:");
        assert_eq!(cmd_prefix("ATD+1555;"), "");
    }
    #[test]
    fn reader() {
        let mut r = LineReader::default();
        let mut q = VecDeque::new();
        r.push(b"\r\nOK\r\n\r\n> ", &mut q);
        assert_eq!(q, VecDeque::from(vec![Item::Line("OK".into()), Item::Prompt]));
    }
}
