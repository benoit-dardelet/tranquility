//! Simulated baseband + mobile network.
//!
//! [`SimModem`] implements [`Link`]: it speaks the AT command set over an
//! in-memory byte pipe exactly the way a UART-attached LTE/NR module would,
//! and behind it sits a tiny network: registration (E-UTRAN / NG-RAN),
//! voice calls, an SMSC, a USSD gateway, cell broadcast, packet data with
//! TCP sockets, an MMSC (HTTP) and an IMS core with an RCS messaging AS
//! (SIP). A handful of "bot" subscribers answer calls and reply to
//! messages so every feature can be exercised without real hardware.
use crate::at::Link;
use crate::cb;
use crate::prelude::*;
use crate::sms::{self, Tpdu};
use crate::util::{self, hex, split_args, starts_with_ic, unhex, Rng};

pub mod bots;
pub mod ftcs;
pub mod ims;
pub mod mmsc;

pub const OWN_NUMBER: &str = "+15550100";
pub const IMSI: &str = "310260000000100";
pub const IMEI: &str = "356938035643809";
pub const ICCID: &str = "8901260000000001007";
pub const SIM_PIN: &str = "1234";
pub const OPERATOR: &str = "Tranquil Mobile";
pub const SMSC: &str = "+15550000";
pub const IMS_DOMAIN: &str = "ims.tranquil.net";
pub const IMS_PASSWORD: &str = "tranquil-ims-secret";
pub const MMSC_HOST: &str = "mmsc.tranquil.net";
pub const MMSC_URL: &str = "http://mmsc.tranquil.net/mms";
/// Network timezone in quarter hours (UTC-5).
pub const TZ_Q: i8 = -20;

pub fn base_unix() -> i64 {
    util::unix_from(2026, 11, 21, 17, 0, 0)
}

#[derive(Clone, Debug)]
pub enum Ev {
    Urc(String),
    UrcPdu(String, String),
    Register,
    CallState(u8, u8),
    CallEnd(u8, &'static str),
    Ring(u8),
    SockOpen(u8, u16),
    SockData(u8, Vec<u8>),
    SockClosed(u8),
    SmsFrom { from: String, text: String },
    WapPush { from: String, data: Vec<u8> },
    StatusReport { mr: u8, to: String },
    Ims(ims::Timed),
    Mms(mmsc::Timed),
    CellBroadcast(u16, String),
    Ussd(String),
}

pub struct Sched {
    pub now: u64,
    q: Vec<(u64, u64, Ev)>,
    seq: u64,
    pub rng: Rng,
}

impl Sched {
    pub fn at(&mut self, delay_ms: u64, ev: Ev) {
        self.seq += 1;
        self.q.push((self.now + delay_ms, self.seq, ev));
    }
    pub fn unix(&self) -> i64 {
        base_unix() + (self.now / 1000) as i64
    }
    fn pop_due(&mut self) -> Option<Ev> {
        let idx = self
            .q
            .iter()
            .enumerate()
            .filter(|(_, (t, _, _))| *t <= self.now)
            .min_by_key(|(_, (t, s, _))| (*t, *s))
            .map(|(i, _)| i)?;
        Some(self.q.swap_remove(idx).2)
    }
}

#[derive(Clone, Debug)]
struct Call {
    id: u8,
    dir: u8,
    stat: u8,
    number: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Server {
    Ims,
    Mmsc,
    Ftcs,
}

pub struct SimModem {
    s: Sched,
    rx: Vec<u8>,
    out: VecDeque<u8>,
    pdu_mode: Option<usize>,
    echo: bool,
    cfun: u8,
    pin_ok: bool,
    pin_tries: u8,
    reg: u8,
    act: u8,
    ws46: u8,
    cereg_n: u8,
    c5greg_n: u8,
    sms_mr: u8,
    calls: Vec<Call>,
    next_call: u8,
    pdp_defined: [bool; 8],
    pdp_active: [bool; 8],
    sockets: BTreeMap<u8, Server>,
    ims: ims::Ims,
    mmsc: mmsc::Mmsc,
    ftcs: ftcs::Ftcs,
    concat: BTreeMap<(String, u16), Vec<Option<String>>>,
    sms_ref: u8,
    signal_lost_until: u64,
    talker: Option<(String, crate::audio::Talker)>,
}

impl Default for SimModem {
    fn default() -> Self {
        Self::new()
    }
}

impl SimModem {
    pub fn new() -> SimModem {
        SimModem {
            s: Sched { now: 0, q: Vec::new(), seq: 0, rng: Rng::new(0x5EED_CAFE) },
            rx: Vec::new(),
            out: VecDeque::new(),
            pdu_mode: None,
            echo: true,
            cfun: 1,
            pin_ok: false,
            pin_tries: 3,
            reg: 0,
            act: 11,
            ws46: 37,
            cereg_n: 0,
            c5greg_n: 0,
            sms_mr: 0,
            calls: Vec::new(),
            next_call: 1,
            pdp_defined: [false; 8],
            pdp_active: [false; 8],
            sockets: BTreeMap::new(),
            ims: ims::Ims::default(),
            mmsc: mmsc::Mmsc::default(),
            ftcs: ftcs::Ftcs::default(),
            concat: BTreeMap::new(),
            sms_ref: 0,
            signal_lost_until: 0,
            talker: None,
        }
    }

    fn emit(&mut self, s: &str) {
        self.out.extend(b"\r\n");
        self.out.extend(s.as_bytes());
        self.out.extend(b"\r\n");
    }

    fn ok(&mut self) {
        self.emit("OK");
    }

    fn err(&mut self, code: u16) {
        self.emit(&format!("+CME ERROR: {}", code));
    }

    fn cms_err(&mut self, code: u16) {
        self.emit(&format!("+CMS ERROR: {}", code));
    }

    fn registered(&self) -> bool {
        self.reg == 1 && self.cfun == 1
    }

    fn reg_urcs(&mut self) {
        let (lte, nr) = match (self.reg, self.act) {
            (1, 7) => (1, 0),
            (1, _) => (0, 1),
            (r, _) => (r, r),
        };
        if self.cereg_n > 0 {
            self.emit(&format!("+CEREG: {}", lte));
        }
        if self.c5greg_n > 0 {
            self.emit(&format!("+C5GREG: {}", nr));
        }
    }

    fn schedule_register(&mut self) {
        if self.cfun == 1 && self.pin_ok {
            self.reg = 2;
            self.reg_urcs();
            self.s.at(1500, Ev::Register);
        }
    }

    fn drop_network(&mut self) {
        self.reg = 0;
        self.pdp_active = [false; 8];
        let socks: Vec<u8> = self.sockets.keys().copied().collect();
        for c in socks {
            self.close_socket(c);
            self.emit(&format!("+QIURC: \"closed\",{}", c));
        }
        let had_call = !self.calls.is_empty();
        self.calls.clear();
        if had_call {
            self.emit("NO CARRIER");
        }
        self.reg_urcs();
    }

    fn close_socket(&mut self, c: u8) {
        match self.sockets.remove(&c) {
            Some(Server::Ims) => self.ims.on_close(c),
            Some(Server::Mmsc) => self.mmsc.on_close(c),
            Some(Server::Ftcs) => self.ftcs.on_close(c),
            None => {}
        }
    }

    // ------------------------------------------------------------ AT server

    fn command(&mut self, raw: &str) {
        let line = raw.trim();
        if line.is_empty() {
            return;
        }
        if self.echo {
            self.out.extend(line.as_bytes());
            self.out.extend(b"\r");
        }
        if !starts_with_ic(line, "AT") {
            return;
        }
        let body = &line[2..];
        if body.is_empty() {
            return self.ok();
        }
        if body.starts_with('+') {
            let end = body.find(['=', '?']).unwrap_or(body.len());
            let name = body[1..end].to_ascii_uppercase();
            let rest = &body[end..];
            let (read, test, args) = if rest == "?" {
                (true, false, Vec::new())
            } else if rest == "=?" {
                (false, true, Vec::new())
            } else if let Some(a) = rest.strip_prefix('=') {
                (false, false, split_args(a))
            } else {
                (false, false, Vec::new())
            };
            if test {
                return self.ok();
            }
            return self.extended(&name, read, &args, rest.strip_prefix('=').unwrap_or(""));
        }
        let up = body.to_ascii_uppercase();
        match up.as_bytes()[0] {
            b'D' => self.dial(&body[1..]),
            b'A' => self.answer(),
            b'H' => {
                self.calls.clear();
                self.ok()
            }
            b'E' => {
                self.echo = up != "E0";
                self.ok()
            }
            b'I' => {
                self.emit("Tranquil Baseband TQ-5G1");
                self.emit("Revision: TQ5G1R01A02");
                self.ok()
            }
            b'Z' | b'V' | b'Q' => self.ok(),
            _ => self.emit("ERROR"),
        }
    }

    fn need_pin(&mut self) -> bool {
        if !self.pin_ok {
            self.err(11);
            return true;
        }
        false
    }

    fn extended(&mut self, name: &str, read: bool, a: &[String], raw_args: &str) {
        let arg = |i: usize| a.get(i).map(|s| s.as_str()).unwrap_or("");
        let num = |i: usize| a.get(i).and_then(|s| s.parse::<u32>().ok());
        match name {
            "CMEE" | "CSCS" | "CMUT" | "CLIP" | "CSCB" | "CSMS" | "QICFG" | "CTZR" | "CMMS" => self.ok(),
            "CGMI" => {
                self.emit("Tranquil");
                self.ok()
            }
            "CGMM" => {
                self.emit("TQ-5G1");
                self.ok()
            }
            "CGMR" => {
                self.emit("TQ5G1R01A02");
                self.ok()
            }
            "CGSN" => {
                self.emit(IMEI);
                self.ok()
            }
            "CIMI" => {
                if self.need_pin() {
                    return;
                }
                self.emit(IMSI);
                self.ok()
            }
            "QCCID" => {
                self.emit(&format!("+QCCID: {}", ICCID));
                self.ok()
            }
            "CNUM" => {
                if self.need_pin() {
                    return;
                }
                self.emit(&format!("+CNUM: \"\",\"{}\",145", OWN_NUMBER));
                self.ok()
            }
            "CPIN" => {
                if read {
                    let st = if self.pin_ok { "READY" } else if self.pin_tries == 0 { "SIM PUK" } else { "SIM PIN" };
                    self.emit(&format!("+CPIN: {}", st));
                    return self.ok();
                }
                if self.pin_ok {
                    return self.ok();
                }
                if self.pin_tries == 0 {
                    return self.err(12);
                }
                if arg(0) == SIM_PIN {
                    self.pin_ok = true;
                    self.pin_tries = 3;
                    self.ok();
                    self.schedule_register();
                } else {
                    self.pin_tries -= 1;
                    self.err(16);
                }
            }
            "CPINR" => {
                self.emit(&format!("+CPINR: \"SIM PIN\",{},3", self.pin_tries));
                self.ok()
            }
            "CFUN" => {
                if read {
                    self.emit(&format!("+CFUN: {}", self.cfun));
                    return self.ok();
                }
                let n = num(0).unwrap_or(1) as u8;
                self.ok();
                if n != self.cfun {
                    self.cfun = n;
                    if n == 1 {
                        self.schedule_register();
                    } else {
                        self.drop_network();
                    }
                }
            }
            "CEREG" | "C5GREG" | "CREG" => {
                if read {
                    let (lte, nr) = match (self.reg, self.act) {
                        (1, 7) => (1, 0),
                        (1, _) => (0, 1),
                        (r, _) => (r, r),
                    };
                    let (n, st) = if name == "C5GREG" { (self.c5greg_n, nr) } else { (self.cereg_n, lte) };
                    self.emit(&format!("+{}: {},{}", name, n, st));
                    return self.ok();
                }
                let n = num(0).unwrap_or(0) as u8;
                match name {
                    "CEREG" => self.cereg_n = n,
                    "C5GREG" => self.c5greg_n = n,
                    _ => {}
                }
                self.ok()
            }
            "COPS" => {
                if read {
                    if self.registered() {
                        self.emit(&format!("+COPS: 0,0,\"{}\",{}", OPERATOR, self.act));
                    } else {
                        self.emit("+COPS: 0");
                    }
                }
                self.ok()
            }
            "WS46" => {
                if read {
                    self.emit(&format!("+WS46: {}", self.ws46));
                    return self.ok();
                }
                let n = num(0).unwrap_or(37) as u8;
                if !matches!(n, 28 | 36 | 37 | 12 | 25) {
                    return self.err(50);
                }
                self.ok();
                if n != self.ws46 {
                    self.ws46 = n;
                    self.act = if n == 28 || n == 12 || n == 25 { 7 } else { 11 };
                    if self.registered() {
                        self.drop_network();
                        self.schedule_register();
                    }
                }
            }
            "CSQ" => {
                let rssi = if self.registered() { 18 + self.s.rng.below(13) } else { 99 };
                self.emit(&format!("+CSQ: {},99", rssi));
                self.ok()
            }
            "CBC" => {
                let lvl = 100u64.saturating_sub(self.s.now / 300_000).max(5);
                self.emit(&format!("+CBC: 0,{},{}", lvl, 3500 + lvl * 7));
                self.ok()
            }
            "CCLK" => {
                let d = util::civil_from_unix(self.s.unix() + TZ_Q as i64 * 900);
                self.emit(&format!(
                    "+CCLK: \"{:02}/{:02}/{:02},{:02}:{:02}:{:02}{:+03}\"",
                    d.year % 100,
                    d.month,
                    d.day,
                    d.hour,
                    d.min,
                    d.sec,
                    TZ_Q
                ));
                self.ok()
            }
            "CMGF" => {
                if num(0) == Some(1) {
                    return self.cms_err(303); // text mode not supported: PDU only
                }
                self.ok()
            }
            "CNMI" => self.ok(),
            "CMGS" => {
                if self.need_pin() {
                    return;
                }
                let len = num(0).unwrap_or(0) as usize;
                self.pdu_mode = Some(len);
                self.out.extend(b"\r\n> ");
            }
            "CLCC" => {
                let lines: Vec<String> =
                    self.calls.iter().map(|c| format!("+CLCC: {},{},{},0,0,\"{}\",{}", c.id, c.dir, c.stat, c.number, if c.number.starts_with('+') { 145 } else { 129 })).collect();
                for l in lines {
                    self.emit(&l);
                }
                self.ok()
            }
            "CHUP" => {
                self.calls.clear();
                self.ok()
            }
            "VTS" => self.ok(),
            "CUSD" => {
                let code = arg(1).to_string();
                self.ok();
                let text = match code.as_str() {
                    "*100#" => "Balance: $12.34. Plan renews 2026-12-01.".to_string(),
                    "*101#" => "Data left: 42.0 GB of 50 GB (5G).".to_string(),
                    "*102#" => "Your number is +15550100".to_string(),
                    _ => "Unknown service code.".to_string(),
                };
                self.s.at(700, Ev::Ussd(text));
            }
            "CGDCONT" => {
                if read {
                    for i in 1..8 {
                        if self.pdp_defined[i] {
                            self.emit(&format!("+CGDCONT: {},\"IPV4V6\",\"apn{}\"", i, i));
                        }
                    }
                    return self.ok();
                }
                if let Some(cid) = num(0) {
                    if (1..8).contains(&cid) {
                        self.pdp_defined[cid as usize] = true;
                    }
                }
                self.ok()
            }
            "CGACT" => {
                if read {
                    for i in 1..8 {
                        if self.pdp_defined[i] {
                            self.emit(&format!("+CGACT: {},{}", i, self.pdp_active[i] as u8));
                        }
                    }
                    return self.ok();
                }
                let on = num(0) == Some(1);
                let cid = num(1).unwrap_or(1) as usize;
                if cid >= 8 || !self.pdp_defined[cid] {
                    return self.err(50);
                }
                if on && !self.registered() {
                    return self.err(30);
                }
                self.pdp_active[cid] = on;
                self.ok()
            }
            "CGPADDR" => {
                let cid = num(0).unwrap_or(1);
                self.emit(&format!("+CGPADDR: {},\"10.64.0.{}\"", cid, 2 + cid));
                self.ok()
            }
            "QIOPEN" => {
                let ctx = num(0).unwrap_or(1) as usize;
                let conn = num(1).unwrap_or(0) as u8;
                let host = arg(3).to_string();
                if ctx >= 8 || !self.pdp_active[ctx] || self.sockets.contains_key(&conn) || conn > 11 {
                    return self.err(563);
                }
                self.ok();
                let server = match host.as_str() {
                    IMS_DOMAIN => Some(Server::Ims),
                    MMSC_HOST => Some(Server::Mmsc),
                    ftcs::HOST => Some(Server::Ftcs),
                    _ => None,
                };
                match server {
                    Some(s) => {
                        self.sockets.insert(conn, s);
                        self.s.at(60, Ev::SockOpen(conn, 0));
                    }
                    None => self.s.at(400, Ev::SockOpen(conn, 565)),
                }
            }
            "QISENDEX" => {
                let conn = num(0).unwrap_or(99) as u8;
                let data = match unhex(arg(1)) {
                    Some(d) => d,
                    None => return self.emit("ERROR"),
                };
                match self.sockets.get(&conn).copied() {
                    Some(Server::Ims) => {
                        self.emit("SEND OK");
                        self.ims.on_data(conn, &data, &mut self.s);
                    }
                    Some(Server::Mmsc) => {
                        self.emit("SEND OK");
                        self.mmsc.on_data(conn, &data, &mut self.s);
                    }
                    Some(Server::Ftcs) => {
                        self.emit("SEND OK");
                        self.ftcs.on_data(conn, &data, &mut self.s);
                    }
                    None => self.emit("SEND FAIL"),
                }
            }
            "QICLOSE" => {
                let conn = num(0).unwrap_or(99) as u8;
                self.close_socket(conn);
                self.ok()
            }
            "SIMEV" => {
                self.ok();
                self.sim_event(num(0).unwrap_or(0));
            }
            _ => {
                let _ = raw_args;
                self.emit("ERROR")
            }
        }
    }

    fn sim_event(&mut self, n: u32) {
        match n {
            1 => {
                if !self.calls.is_empty() || !self.registered() {
                    return;
                }
                let id = self.alloc_call();
                self.calls.push(Call { id, dir: 1, stat: 4, number: "+15550102".into() });
                self.s.at(100, Ev::Ring(id));
                self.s.at(25_000, Ev::CallEnd(id, "NO CARRIER"));
            }
            2 => self.s.at(200, Ev::SmsFrom { from: "+15550102".into(), text: "Hey, it's Bob. Fancy a coffee later?".into() }),
            3 => self.ims.inject("+15550101", "Hi from Alice over RCS! Typing indicators and read receipts work here.", &mut self.s),
            4 => self.mmsc.inject_from_bot("+15550102", &mut self.s),
            5 => self.s.at(300, Ev::CellBroadcast(4370, "Presidential Alert: This is a test of the Wireless Emergency Alert system. No action required.".into())),
            6 => {
                if self.registered() {
                    self.drop_network();
                    self.signal_lost_until = self.s.now + 8000;
                    self.s.at(8000, Ev::Register);
                    self.reg = 2;
                }
            }
            8 => self.ims.inject_group(&mut self.s),
            7 => {
                let text = "Carol here. This is a long message to show concatenated SMS: segments are sent with a user data header carrying a reference number, the total count and the sequence number, and the phone reassembles them in order before showing them. Neat, right?";
                self.s.at(200, Ev::SmsFrom { from: "+15550103".into(), text: text.into() })
            }
            _ => {}
        }
    }

    fn alloc_call(&mut self) -> u8 {
        let id = self.next_call;
        self.next_call = self.next_call % 7 + 1;
        id
    }

    fn dial(&mut self, s: &str) {
        let number: String = s.trim().trim_end_matches(';').chars().filter(|c| c.is_ascii_digit() || matches!(c, '+' | '*' | '#')).collect();
        let emergency = matches!(number.as_str(), "112" | "911" | "999" | "000" | "08" | "110");
        // Emergency calls are allowed in airplane mode and without a SIM PIN.
        if (self.cfun != 1 && !(emergency && self.cfun == 4)) || number.is_empty() {
            return self.emit("NO CARRIER");
        }
        if !emergency && (!self.registered() || !self.pin_ok) {
            return self.err(30);
        }
        if !self.calls.is_empty() {
            return self.err(3);
        }
        let id = self.alloc_call();
        self.calls.push(Call { id, dir: 0, stat: 2, number: number.clone() });
        self.ok();
        self.s.at(800, Ev::CallState(id, 3));
        let b = bots::find(&number);
        if number == "+15550199" {
            self.s.at(1600, Ev::CallEnd(id, "BUSY"));
        } else if emergency {
            self.s.at(1500, Ev::CallState(id, 0));
        } else if let Some(b) = b {
            if b.answers_calls {
                self.s.at(3000, Ev::CallState(id, 0));
                if let Some(t) = b.hangs_up_after {
                    self.s.at(3000 + t, Ev::CallEnd(id, "NO CARRIER"));
                }
            } else {
                self.s.at(15000, Ev::CallEnd(id, "NO ANSWER"));
            }
        } else {
            self.s.at(4000, Ev::CallState(id, 0));
        }
    }

    fn answer(&mut self) {
        match self.calls.iter_mut().find(|c| c.stat == 4) {
            Some(c) => {
                c.stat = 0;
                let id = c.id;
                self.ok();
                // Bob talks for a while then hangs up.
                self.s.at(20_000, Ev::CallEnd(id, "NO CARRIER"));
            }
            None => self.emit("NO CARRIER"),
        }
    }

    fn sms_submit(&mut self, hexpdu: &str, declared_len: usize) {
        self.pdu_mode = None;
        let data = match unhex(hexpdu.trim()) {
            Some(d) => d,
            None => return self.cms_err(304),
        };
        let tp_len = data.len().saturating_sub(1 + data.first().copied().unwrap_or(0) as usize);
        if tp_len != declared_len {
            return self.cms_err(304);
        }
        if !self.registered() {
            return self.cms_err(331);
        }
        let sub = match sms::decode(&data) {
            Some(Tpdu::Submit(s)) => s,
            _ => return self.cms_err(304),
        };
        self.sms_mr = self.sms_mr.wrapping_add(1);
        let mr = self.sms_mr;
        self.emit(&format!("+CMGS: {}", mr));
        self.ok();
        let to = util::normalize_number(&sub.to);
        if sub.srr {
            self.s.at(1500, Ev::StatusReport { mr, to: to.clone() });
        }
        // Reassemble long messages before handing them to a recipient.
        let text = match sub.udh.concat {
            Some(c) if c.total > 1 => {
                let key = (to.clone(), c.reference);
                let slots = self.concat.entry(key.clone()).or_insert_with(|| vec![None; c.total as usize]);
                if let Some(slot) = slots.get_mut((c.seq as usize).wrapping_sub(1)) {
                    *slot = Some(sub.text);
                }
                if slots.iter().any(|x| x.is_none()) {
                    return;
                }
                self.concat.remove(&key).unwrap_or_default().into_iter().flatten().collect::<String>()
            }
            _ => sub.text,
        };
        if util::same_number(&to, OWN_NUMBER) {
            self.s.at(1200, Ev::SmsFrom { from: OWN_NUMBER.into(), text });
        } else if let Some(reply) = bots::find(&to).and_then(|b| bots::reply(b, &text, false)) {
            self.s.at(3000, Ev::SmsFrom { from: to, text: reply });
        }
    }

    fn deliver_sms_text(&mut self, from: &str, text: &str) {
        self.sms_ref = self.sms_ref.wrapping_add(1);
        for tp in sms::build_text_delivers(from, text, self.sms_ref, self.s.unix(), TZ_Q) {
            self.deliver_tpdu(&tp);
        }
    }

    fn deliver_tpdu(&mut self, tp: &[u8]) {
        let mut pdu = sms::encode_smsc(Some(SMSC));
        pdu.extend_from_slice(tp);
        self.emit(&format!("+CMT: ,{}", tp.len()));
        self.out.extend(hex(&pdu).as_bytes());
        self.out.extend(b"\r\n");
    }

    fn event(&mut self, ev: Ev) {
        match ev {
            Ev::Urc(s) => self.emit(&s),
            Ev::UrcPdu(l, p) => {
                self.emit(&l);
                self.out.extend(p.as_bytes());
                self.out.extend(b"\r\n");
            }
            Ev::Register => {
                if self.cfun == 1 && self.pin_ok && self.s.now >= self.signal_lost_until {
                    self.reg = 1;
                    self.reg_urcs();
                }
            }
            Ev::CallState(id, stat) => {
                if let Some(c) = self.calls.iter_mut().find(|c| c.id == id && c.dir == 0) {
                    if c.stat != 0 {
                        c.stat = stat;
                    }
                }
            }
            Ev::CallEnd(id, why) => {
                let before = self.calls.len();
                self.calls.retain(|c| c.id != id);
                if self.calls.len() != before {
                    self.emit(why);
                }
            }
            Ev::Ring(id) => {
                if let Some(c) = self.calls.iter().find(|c| c.id == id && c.stat == 4) {
                    let n = c.number.clone();
                    self.emit("RING");
                    self.emit(&format!("+CLIP: \"{}\",145,,,,0", n));
                    self.s.at(3000, Ev::Ring(id));
                }
            }
            Ev::SockOpen(c, err) => self.emit(&format!("+QIOPEN: {},{}", c, err)),
            Ev::SockData(c, data) => {
                if self.sockets.contains_key(&c) {
                    for chunk in data.chunks(512) {
                        let l = format!("+QIURC: \"recv\",{},{},\"{}\"", c, chunk.len(), hex(chunk));
                        self.emit(&l);
                    }
                }
            }
            Ev::SockClosed(c) => {
                if self.sockets.remove(&c).is_some() {
                    self.emit(&format!("+QIURC: \"closed\",{}", c));
                }
            }
            Ev::SmsFrom { from, text } => {
                if self.registered() {
                    self.deliver_sms_text(&from, &text);
                } else {
                    self.s.at(3000, Ev::SmsFrom { from, text });
                }
            }
            Ev::WapPush { from, data } => {
                if !self.registered() {
                    return self.s.at(3000, Ev::WapPush { from, data });
                }
                self.sms_ref = self.sms_ref.wrapping_add(1);
                let ports = (crate::mms::WAP_PUSH_PORT, crate::mms::WAP_PUSH_SRC_PORT);
                for tp in sms::build_port_delivers(&from, &data, ports, self.sms_ref, self.s.unix(), TZ_Q) {
                    self.deliver_tpdu(&tp);
                }
            }
            Ev::StatusReport { mr, to } => {
                let t = self.s.unix();
                let tp = sms::encode_status_report(&sms::StatusReport { mr, recipient: to, scts: t - 1, discharge: t, status: 0 }, TZ_Q);
                let mut pdu = sms::encode_smsc(Some(SMSC));
                pdu.extend_from_slice(&tp);
                self.emit(&format!("+CDS: {}", tp.len()));
                self.out.extend(hex(&pdu).as_bytes());
                self.out.extend(b"\r\n");
            }
            Ev::Ims(t) => self.ims.timed(t, &mut self.s, &mut self.ftcs),
            Ev::Mms(t) => self.mmsc.timed(t, &mut self.s),
            Ev::CellBroadcast(id, text) => {
                if self.registered() {
                    let page = cb::encode(id, 0x3001, &text);
                    self.emit(&format!("+CBM: {}", page.len()));
                    self.out.extend(hex(&page).as_bytes());
                    self.out.extend(b"\r\n");
                }
            }
            Ev::Ussd(text) => self.emit(&format!("+CUSD: 0,\"{}\",15", text)),
        }
    }
}

impl Link for SimModem {
    fn write(&mut self, data: &[u8]) {
        for &b in data {
            if let Some(len) = self.pdu_mode {
                match b {
                    0x1A => {
                        let s = String::from_utf8_lossy(&self.rx).into_owned();
                        self.rx.clear();
                        self.sms_submit(&s, len);
                    }
                    0x1B => {
                        self.rx.clear();
                        self.pdu_mode = None;
                        self.ok();
                    }
                    b'\r' | b'\n' => {}
                    _ => self.rx.push(b),
                }
                continue;
            }
            match b {
                b'\r' => {
                    let s = String::from_utf8_lossy(&self.rx).into_owned();
                    self.rx.clear();
                    self.command(&s);
                }
                b'\n' => {}
                _ => self.rx.push(b),
            }
        }
    }

    fn read(&mut self, out: &mut Vec<u8>) {
        out.extend(self.out.drain(..));
    }

    fn voice(&mut self, out: &mut [i16]) -> bool {
        let who = match self.calls.iter().find(|c| c.stat == 0) {
            Some(c) => c.number.clone(),
            None => return false,
        };
        if self.talker.as_ref().map(|(n, _)| n != &who).unwrap_or(true) {
            let f0 = match bots::find(&who).map(|b| b.name) {
                Some("Alice") => 215.0,
                Some("Carol") => 240.0,
                Some("Bob") => 115.0,
                _ => 140.0,
            };
            let seed = who.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
            self.talker = Some((who, crate::audio::Talker::new(f0, seed)));
        }
        if let Some((_, t)) = &mut self.talker {
            t.fill(out);
        }
        true
    }

    fn poll(&mut self, now_ms: u64) {
        if now_ms > self.s.now {
            self.s.now = now_ms;
        }
        for _ in 0..256 {
            match self.s.pop_due() {
                Some(ev) => self.event(ev),
                None => break,
            }
        }
    }
}
