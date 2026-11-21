//! Phone system services: network/registration, telephony, SMS, sockets,
//! MMS user agent and RCS client. All of it runs on top of the AT driver.
use crate::at::{Link, Modem, Urc};
use crate::cb;
use crate::mms::Part;
use crate::prelude::*;
use crate::sms::{self, Tpdu};
use crate::util::{self, hex, same_number, split_args, unhex, Rng};

pub mod mms;
pub mod rcs;

pub const IMS_HOST: &str = "ims.tranquil.net";
pub const MMSC_HOST: &str = "mmsc.tranquil.net";
pub const MMSC_PATH: &str = "/mms";
pub const CID_INTERNET: u8 = 1;
pub const CID_IMS: u8 = 2;
pub const CID_MMS: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    Sms,
    Mms,
    Rcs,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Transport::Sms => "SMS",
            Transport::Mms => "MMS",
            Transport::Rcs => "Chat",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Sending,
    Failed,
    Sent,
    Delivered,
    Read,
    Received,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub id: u32,
    pub incoming: bool,
    pub transport: Transport,
    pub text: String,
    pub subject: Option<String>,
    pub attachments: Vec<Part>,
    pub time: i64,
    pub status: Status,
    pub unread: bool,
    /// RCS imdn Message-ID (incoming or outgoing)
    pub imdn_id: String,
    /// sender requested display notification
    pub wants_display: bool,
    /// SMS message references awaiting status reports
    pub sms_mrs: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub number: String,
    pub msgs: Vec<Message>,
    pub typing_until: u64,
}

impl Thread {
    pub fn unread(&self) -> usize {
        self.msgs.iter().filter(|m| m.unread).count()
    }
    pub fn last_time(&self) -> i64 {
        self.msgs.last().map(|m| m.time).unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallState {
    Dialing,
    Alerting,
    Active,
    Incoming,
}

#[derive(Clone, Debug)]
pub struct Call {
    pub number: String,
    pub state: CallState,
    pub incoming: bool,
    pub started: u64,
    pub connected_at: Option<u64>,
    pub muted: bool,
    pub speaker: bool,
    pub emergency: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    Dialed,
    Received,
    Missed,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub number: String,
    pub kind: LogKind,
    pub time: i64,
    pub secs: u64,
}

#[derive(Clone, Debug)]
pub struct Contact {
    pub name: String,
    pub number: String,
}

#[derive(Clone, Debug, Default)]
pub struct DeviceInfo {
    pub manufacturer: String,
    pub model: String,
    pub firmware: String,
    pub imei: String,
    pub imsi: String,
    pub iccid: String,
    pub number: String,
}

#[derive(Clone, Debug, Default)]
pub struct NetState {
    pub registered: bool,
    pub searching: bool,
    /// 27.007 AcT: 7 = E-UTRAN (4G), 11/12/13 = NR (5G)
    pub act: u8,
    pub operator: String,
    pub rssi: u8,
    pub battery: u8,
    pub pdp_up: bool,
}

impl NetState {
    pub fn rat(&self) -> &'static str {
        if !self.registered {
            return if self.searching { "…" } else { "--" };
        }
        match self.act {
            7 => "4G",
            10..=13 => "5G",
            2 | 4..=6 => "3G",
            _ => "2G",
        }
    }
    pub fn bars(&self) -> u8 {
        if !self.registered || self.rssi == 99 {
            0
        } else {
            match self.rssi {
                0..=9 => 1,
                10..=14 => 2,
                15..=19 => 3,
                _ => 4,
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetMode {
    Auto5g,
    Lte,
    Nr,
}

impl NetMode {
    pub fn ws46(self) -> u8 {
        match self {
            NetMode::Auto5g => 37,
            NetMode::Lte => 28,
            NetMode::Nr => 36,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            NetMode::Auto5g => "5G/4G auto",
            NetMode::Lte => "4G only",
            NetMode::Nr => "5G only",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub rcs: bool,
    pub delivery_reports: bool,
    pub read_receipts: bool,
    pub typing_indicators: bool,
    pub airplane: bool,
    pub net_mode: NetMode,
    pub alarm: Option<(u32, u32)>,
    pub alarm_on: bool,
    pub ims_password: String,
    pub ringer: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            rcs: true,
            delivery_reports: true,
            read_receipts: true,
            typing_indicators: true,
            airplane: false,
            net_mode: NetMode::Auto5g,
            alarm: Some((7, 0)),
            alarm_on: false,
            ims_password: crate::sim::IMS_PASSWORD.into(),
            ringer: true,
        }
    }
}

/// Events for the UI layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    IncomingCall(String),
    CallEnded { number: String, secs: u64, reason: String },
    NewMessage { number: String, transport: Transport },
    Ussd(String),
    Alert { title: String, text: String },
    Alarm,
    Toast(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Rcs,
    Mms(u32),
}

#[derive(Debug)]
pub struct Sock {
    pub owner: Owner,
    pub connected: bool,
    pub txq: Vec<u8>,
}

pub struct Core {
    pub now: u64,
    pub modem: Modem,
    pub info: DeviceInfo,
    pub net: NetState,
    pub settings: Settings,
    pub sim_locked: bool,
    pub sim_ready: bool,
    pub pin_tries: u8,
    /// unix time at `now == 0`, from the network clock
    pub epoch: i64,
    pub tz_q: i8,
    pub call: Option<Call>,
    pub log: Vec<LogEntry>,
    pub missed: usize,
    pub contacts: Vec<Contact>,
    pub threads: Vec<Thread>,
    pub events: VecDeque<Event>,
    pub rcs: rcs::Rcs,
    pub mms: mms::MmsAgent,
    socks: BTreeMap<u8, Sock>,
    reasm: sms::Reassembler,
    next_msg: u32,
    pub rng: Rng,
    last_csq: u64,
    last_cbc: u64,
    last_clcc: u64,
    last_minute: i64,
    pub alarm_snooze: Option<i64>,
}

impl Core {
    pub fn new(link: Box<dyn Link>, clock: crate::at::Clock) -> Core {
        let mut c = Core {
            now: clock(),
            modem: Modem::new(link, clock),
            info: DeviceInfo::default(),
            net: NetState { rssi: 99, battery: 100, ..Default::default() },
            settings: Settings::default(),
            sim_locked: false,
            sim_ready: false,
            pin_tries: 3,
            epoch: crate::sim::base_unix(),
            tz_q: 0,
            call: None,
            log: Vec::new(),
            missed: 0,
            contacts: Vec::new(),
            threads: Vec::new(),
            events: VecDeque::new(),
            rcs: rcs::Rcs::default(),
            mms: mms::MmsAgent::default(),
            socks: BTreeMap::new(),
            reasm: sms::Reassembler::default(),
            next_msg: 1,
            rng: Rng::new(0x7A4E_0001 ^ clock()),
            last_csq: 0,
            last_cbc: 0,
            last_clcc: 0,
            last_minute: 0,
            alarm_snooze: None,
        };
        for (n, num) in [("Alice", "+15550101"), ("Bob", "+15550102"), ("Carol", "+15550103"), ("Dave", "+15550104"), ("Voicemail", "+15550199")] {
            c.contacts.push(Contact { name: n.into(), number: num.into() });
        }
        c
    }

    /// Current UTC unix time.
    pub fn unix(&self) -> i64 {
        self.epoch + (self.now / 1000) as i64
    }

    /// Current local unix time (for display).
    pub fn local(&self) -> i64 {
        self.unix() + self.tz_q as i64 * 900
    }

    pub fn name_for(&self, number: &str) -> String {
        self.contacts.iter().find(|c| same_number(&c.number, number)).map(|c| c.name.clone()).unwrap_or_else(|| number.to_string())
    }

    pub fn toast(&mut self, s: &str) {
        self.events.push_back(Event::Toast(s.to_string()));
    }

    // ------------------------------------------------------------ boot

    pub fn boot(&mut self) {
        self.modem.cmd("ATE0");
        self.modem.cmd("AT+CMEE=1");
        let r = self.modem.cmd("AT+CGMI");
        self.info.manufacturer = r.first().unwrap_or("").to_string();
        let r = self.modem.cmd("AT+CGMM");
        self.info.model = r.first().unwrap_or("").to_string();
        let r = self.modem.cmd("AT+CGMR");
        self.info.firmware = r.first().unwrap_or("").to_string();
        let r = self.modem.cmd("AT+CGSN");
        self.info.imei = r.first().unwrap_or("").to_string();
        if let Some(v) = self.modem.cmd("AT+QCCID").get("+QCCID:") {
            self.info.iccid = v.to_string();
        }
        self.modem.cmd("AT+CEREG=1");
        self.modem.cmd("AT+C5GREG=1");
        self.modem.cmd("AT+CLIP=1");
        self.modem.cmd("AT+CMGF=0");
        self.modem.cmd("AT+CNMI=2,2,0,1,0");
        self.modem.cmd("AT+CSCB=0,\"\",\"\"");
        self.modem.cmd("AT+CTZR=1");
        self.sync_clock();
        let r = self.modem.cmd("AT+CPIN?");
        match r.get("+CPIN:") {
            Some("READY") => self.sim_unlocked(),
            Some(_) => {
                self.sim_locked = true;
                self.update_pin_tries();
            }
            None => self.toast("No SIM"),
        }
    }

    fn update_pin_tries(&mut self) {
        if let Some(v) = self.modem.cmd("AT+CPINR=\"SIM PIN\"").get("+CPINR:") {
            if let Some(n) = split_args(v).get(1).and_then(|s| s.parse().ok()) {
                self.pin_tries = n;
            }
        }
    }

    /// Returns true if the PIN was accepted.
    pub fn enter_pin(&mut self, pin: &str) -> bool {
        let r = self.modem.cmd(&format!("AT+CPIN=\"{}\"", pin));
        if r.ok {
            self.sim_unlocked();
            true
        } else {
            self.update_pin_tries();
            false
        }
    }

    fn sim_unlocked(&mut self) {
        self.sim_locked = false;
        self.sim_ready = true;
        self.info.imsi = self.modem.cmd("AT+CIMI").first().unwrap_or("").to_string();
        if let Some(v) = self.modem.cmd("AT+CNUM").get("+CNUM:") {
            self.info.number = split_args(v).get(1).cloned().unwrap_or_default();
        }
        for (cid, apn) in [(CID_INTERNET, "internet"), (CID_IMS, "ims"), (CID_MMS, "mms")] {
            self.modem.cmd(&format!("AT+CGDCONT={},\"IPV4V6\",\"{}\"", cid, apn));
        }
        self.modem.cmd(&format!("AT+WS46={}", self.settings.net_mode.ws46()));
        self.modem.cmd(if self.settings.airplane { "AT+CFUN=4" } else { "AT+CFUN=1" });
        self.query_reg();
    }

    fn sync_clock(&mut self) {
        // +CCLK: "yy/MM/dd,hh:mm:ss±zz" (local time, zz in quarter hours)
        let r = self.modem.cmd("AT+CCLK?");
        if let Some(v) = r.get("+CCLK:") {
            let v = v.trim_matches('"');
            let n: Vec<i64> = v.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
            if n.len() >= 7 {
                let tz = if v.contains('-') && v.rfind('-') > v.find(',') { -n[6] } else { n[6] };
                let local = util::unix_from(2000 + n[0], n[1] as u32, n[2] as u32, n[3] as u32, n[4] as u32, n[5] as u32);
                self.tz_q = tz as i8;
                self.epoch = local - tz * 900 - (self.now / 1000) as i64;
            }
        }
    }

    fn query_reg(&mut self) {
        let lte = self.modem.cmd("AT+CEREG?").get("+CEREG:").and_then(|v| split_args(v).get(1).and_then(|s| s.parse::<u8>().ok())).unwrap_or(0);
        let nr = self.modem.cmd("AT+C5GREG?").get("+C5GREG:").and_then(|v| split_args(v).get(1).and_then(|s| s.parse::<u8>().ok())).unwrap_or(0);
        self.on_reg(lte.max(nr) == 1 || lte == 5 || nr == 5, lte == 2 || nr == 2);
    }

    fn on_reg(&mut self, registered: bool, searching: bool) {
        let was = self.net.registered;
        self.net.searching = searching;
        if registered && !was {
            self.net.registered = true;
            let r = self.modem.cmd("AT+COPS?");
            if let Some(v) = r.get("+COPS:") {
                let a = split_args(v);
                self.net.operator = a.get(2).cloned().unwrap_or_default();
                self.net.act = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(7);
            }
            self.refresh_signal();
            let mut up = true;
            for cid in [CID_INTERNET, CID_IMS, CID_MMS] {
                up &= self.modem.cmd(&format!("AT+CGACT=1,{}", cid)).ok;
            }
            self.net.pdp_up = up;
            self.rcs_start();
        } else if !registered && was {
            self.net.registered = false;
            self.net.pdp_up = false;
            self.net.rssi = 99;
            self.socks.clear();
            self.rcs_reset();
            self.mms_reset();
        }
    }

    fn refresh_signal(&mut self) {
        if let Some(v) = self.modem.cmd("AT+CSQ").get("+CSQ:") {
            self.net.rssi = split_args(v).first().and_then(|s| s.parse().ok()).unwrap_or(99);
        }
    }

    // ------------------------------------------------------------ main loop

    pub fn step(&mut self, now: u64) {
        self.now = now;
        self.modem.poll();
        while let Some(u) = self.modem.take_urc() {
            self.on_urc(u);
        }
        if self.call.is_some() && now.saturating_sub(self.last_clcc) >= 500 {
            self.last_clcc = now;
            self.poll_calls();
        }
        if self.sim_ready && now.saturating_sub(self.last_csq) >= 10_000 {
            self.last_csq = now;
            if self.net.registered {
                self.refresh_signal();
            }
        }
        if now.saturating_sub(self.last_cbc) >= 60_000 || self.last_cbc == 0 {
            self.last_cbc = now.max(1);
            if let Some(v) = self.modem.cmd("AT+CBC").get("+CBC:") {
                self.net.battery = split_args(v).get(1).and_then(|s| s.parse().ok()).unwrap_or(100);
            }
        }
        self.rcs_tick();
        self.mms_tick();
        for t in &mut self.threads {
            if t.typing_until != 0 && t.typing_until < now {
                t.typing_until = 0;
            }
        }
        self.alarm_tick();
    }

    fn alarm_tick(&mut self) {
        let minute = self.local() / 60;
        if minute == self.last_minute {
            return;
        }
        self.last_minute = minute;
        let d = util::civil_from_unix(self.local());
        let due_alarm = self.settings.alarm_on && self.settings.alarm == Some((d.hour, d.min));
        let due_snooze = self.alarm_snooze.is_some_and(|t| minute >= t / 60);
        if due_alarm || due_snooze {
            self.alarm_snooze = None;
            self.events.push_back(Event::Alarm);
        }
    }

    fn on_urc(&mut self, u: Urc) {
        let l = u.line.as_str();
        let (name, val) = match l.split_once(':') {
            Some((n, v)) => (n.trim(), v.trim()),
            None => (l, ""),
        };
        match name {
            "+CEREG" | "+C5GREG" => {
                let st: u8 = split_args(val).first().and_then(|s| s.parse().ok()).unwrap_or(0);
                if st == 1 || st == 5 {
                    self.on_reg(true, false);
                } else {
                    // the other RAT reporting "not registered" is fine if one is up
                    self.query_reg();
                }
            }
            "RING" | "+CRING" => {
                if self.call.is_none() {
                    self.call = Some(Call { number: String::new(), state: CallState::Incoming, incoming: true, started: self.now, connected_at: None, muted: false, speaker: false, emergency: false });
                }
            }
            "+CLIP" => {
                let num = split_args(val).first().cloned().unwrap_or_default();
                let fire = match &mut self.call {
                    Some(c) if c.state == CallState::Incoming && c.number.is_empty() => {
                        c.number = num.clone();
                        true
                    }
                    _ => false,
                };
                if fire {
                    self.events.push_back(Event::IncomingCall(num));
                }
            }
            "NO CARRIER" | "BUSY" | "NO ANSWER" => self.call_ended(name),
            "+CMT" => {
                if let Some(p) = u.pdu.as_deref().and_then(unhex) {
                    self.on_sms_pdu(&p);
                }
            }
            "+CDS" => {
                if let Some(Tpdu::Status(r)) = u.pdu.as_deref().and_then(unhex).and_then(|p| sms::decode(&p)) {
                    self.on_status_report(r);
                }
            }
            "+CBM" => {
                if let Some(m) = u.pdu.as_deref().and_then(unhex).and_then(|p| cb::decode(&p)) {
                    self.events.push_back(Event::Alert { title: cb::category(m.msg_id).into(), text: m.text });
                }
            }
            "+CUSD" => {
                let a = split_args(val);
                let text = a.get(1).cloned().unwrap_or_else(|| "USSD session ended".into());
                self.events.push_back(Event::Ussd(text));
            }
            "+QIOPEN" => {
                let a = split_args(val);
                let id: u8 = a.first().and_then(|s| s.parse().ok()).unwrap_or(255);
                let err: u32 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                self.on_sock_open(id, err == 0);
            }
            "+QIURC" => {
                let a = split_args(val);
                let id: u8 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(255);
                match a.first().map(|s| s.as_str()) {
                    Some("recv") => {
                        if let Some(d) = a.get(3).and_then(|h| unhex(h)) {
                            self.on_sock_data(id, &d);
                        }
                    }
                    Some("closed") => self.on_sock_closed(id),
                    _ => {}
                }
            }
            "+CTZE" | "+CTZV" => self.sync_clock(),
            _ => {}
        }
    }

    // ------------------------------------------------------------ telephony

    pub fn is_emergency(n: &str) -> bool {
        matches!(n, "112" | "911" | "999" | "000" | "08" | "110")
    }

    pub fn dial(&mut self, number: &str) -> Result<(), String> {
        let number = util::normalize_number(number);
        if number.is_empty() {
            return Err("No number".into());
        }
        if self.call.is_some() {
            return Err("Call in progress".into());
        }
        let emergency = Self::is_emergency(&number);
        if self.settings.airplane && !emergency {
            return Err("Airplane mode".into());
        }
        let r = self.modem.cmd(&format!("ATD{};", number));
        if !r.ok {
            let why = if r.error.as_deref() == Some("+CME ERROR: 30") { "No network".to_string() } else { r.error.unwrap_or_default() };
            self.log.push(LogEntry { number, kind: LogKind::Dialed, time: self.unix(), secs: 0 });
            return Err(why);
        }
        self.call = Some(Call { number, state: CallState::Dialing, incoming: false, started: self.now, connected_at: None, muted: false, speaker: false, emergency });
        self.last_clcc = self.now;
        Ok(())
    }

    pub fn answer(&mut self) {
        if matches!(&self.call, Some(c) if c.state == CallState::Incoming) && self.modem.cmd("ATA").ok {
            if let Some(c) = &mut self.call {
                c.state = CallState::Active;
                c.connected_at = Some(self.now);
            }
        }
    }

    pub fn hangup(&mut self) {
        if self.call.is_some() {
            self.modem.cmd("AT+CHUP");
            self.call_ended("Call ended");
        }
    }

    pub fn dtmf(&mut self, d: char) {
        if matches!(&self.call, Some(c) if c.state == CallState::Active) {
            self.modem.cmd(&format!("AT+VTS={}", d));
        }
    }

    pub fn toggle_mute(&mut self) {
        let m = match &mut self.call {
            Some(c) => {
                c.muted = !c.muted;
                c.muted
            }
            None => return,
        };
        self.modem.cmd(&format!("AT+CMUT={}", m as u8));
    }

    fn poll_calls(&mut self) {
        let r = self.modem.cmd("AT+CLCC");
        if !r.ok {
            return;
        }
        let lines = r.all("+CLCC:");
        if lines.is_empty() {
            if self.call.as_ref().is_some_and(|c| c.state != CallState::Incoming || !c.number.is_empty()) {
                self.call_ended("Call ended");
            }
            return;
        }
        let a = split_args(lines[0]);
        let stat: u8 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        let num = a.get(5).cloned().unwrap_or_default();
        let now = self.now;
        let mut ring = None;
        if let Some(c) = &mut self.call {
            if c.number.is_empty() {
                c.number = num.clone();
                if c.state == CallState::Incoming {
                    ring = Some(num);
                }
            }
            c.state = match stat {
                0 | 1 => CallState::Active,
                2 => CallState::Dialing,
                3 => CallState::Alerting,
                _ => CallState::Incoming,
            };
            if c.state == CallState::Active && c.connected_at.is_none() {
                c.connected_at = Some(now);
            }
        }
        if let Some(n) = ring {
            self.events.push_back(Event::IncomingCall(n));
        }
    }

    fn call_ended(&mut self, reason: &str) {
        let c = match self.call.take() {
            Some(c) => c,
            None => return,
        };
        let secs = c.connected_at.map(|t| (self.now - t) / 1000).unwrap_or(0);
        let kind = match (c.incoming, c.connected_at.is_some()) {
            (false, _) => LogKind::Dialed,
            (true, true) => LogKind::Received,
            (true, false) => LogKind::Missed,
        };
        if kind == LogKind::Missed {
            self.missed += 1;
        }
        self.log.push(LogEntry { number: c.number.clone(), kind, time: self.unix() - secs as i64, secs });
        let reason = match reason {
            "BUSY" => "Busy",
            "NO ANSWER" => "No answer",
            "NO CARRIER" => "Call ended",
            r => r,
        };
        self.events.push_back(Event::CallEnded { number: c.number, secs, reason: reason.into() });
    }

    pub fn ussd(&mut self, code: &str) {
        let r = self.modem.cmd(&format!("AT+CUSD=1,\"{}\",15", code));
        if !r.ok {
            self.events.push_back(Event::Ussd("USSD request failed".into()));
        }
    }

    // ------------------------------------------------------------ settings

    pub fn set_airplane(&mut self, on: bool) {
        self.settings.airplane = on;
        if self.sim_ready {
            self.modem.cmd(if on { "AT+CFUN=4" } else { "AT+CFUN=1" });
            if on {
                self.on_reg(false, false);
            }
        }
    }

    pub fn set_net_mode(&mut self, m: NetMode) {
        self.settings.net_mode = m;
        if self.sim_ready {
            self.modem.cmd(&format!("AT+WS46={}", m.ws46()));
        }
    }

    pub fn set_rcs(&mut self, on: bool) {
        self.settings.rcs = on;
        if on {
            self.rcs_start();
        } else {
            self.rcs_stop();
        }
    }

    // ------------------------------------------------------------ messages

    pub fn thread_idx(&mut self, number: &str) -> usize {
        if let Some(i) = self.threads.iter().position(|t| same_number(&t.number, number)) {
            return i;
        }
        self.threads.push(Thread { number: number.to_string(), msgs: Vec::new(), typing_until: 0 });
        self.threads.len() - 1
    }

    pub fn find_msg(&mut self, id: u32) -> Option<&mut Message> {
        self.threads.iter_mut().flat_map(|t| t.msgs.iter_mut()).find(|m| m.id == id)
    }

    fn new_msg(&mut self, incoming: bool, transport: Transport, text: &str) -> Message {
        let id = self.next_msg;
        self.next_msg += 1;
        Message {
            id,
            incoming,
            transport,
            text: text.to_string(),
            subject: None,
            attachments: Vec::new(),
            time: self.unix(),
            status: if incoming { Status::Received } else { Status::Sending },
            unread: incoming,
            imdn_id: String::new(),
            wants_display: false,
            sms_mrs: Vec::new(),
        }
    }

    pub fn push_incoming(&mut self, number: &str, mut m: Message) {
        let t = self.thread_idx(number);
        m.unread = true;
        self.threads[t].typing_until = 0;
        let transport = m.transport;
        self.threads[t].msgs.push(m);
        self.events.push_back(Event::NewMessage { number: number.to_string(), transport });
    }

    /// Send a message, choosing RCS → SMS (or MMS for attachments).
    pub fn send_message(&mut self, to: &str, text: &str, attachments: Vec<Part>, force_sms: bool) -> u32 {
        let to = util::normalize_number(to);
        let transport = if !attachments.is_empty() {
            Transport::Mms
        } else if !force_sms && self.rcs_usable() {
            Transport::Rcs
        } else {
            Transport::Sms
        };
        let mut m = self.new_msg(false, transport, text);
        m.attachments = attachments.clone();
        let id = m.id;
        let t = self.thread_idx(&to);
        self.threads[t].msgs.push(m);
        match transport {
            Transport::Sms => self.send_sms(id, &to, text),
            Transport::Mms => self.mms_send(id, &to, text, attachments),
            Transport::Rcs => self.rcs_send(id, &to, text),
        }
        id
    }

    pub fn send_sms(&mut self, id: u32, to: &str, text: &str) {
        let reference = self.rng.below(255) as u8 + 1;
        let srr = self.settings.delivery_reports;
        let mut mrs = Vec::new();
        let mut ok = self.net.registered;
        if ok {
            for tp in sms::build_submits(to, text, srr, reference) {
                let mut pdu = vec![0u8];
                pdu.extend_from_slice(&tp);
                let mut payload = hex(&pdu).into_bytes();
                payload.push(0x1A);
                let r = self.modem.cmd_payload(&format!("AT+CMGS={}", tp.len()), &payload);
                match r.get("+CMGS:").and_then(|v| v.parse::<u8>().ok()) {
                    Some(mr) if r.ok => mrs.push(mr),
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
        }
        if let Some(m) = self.find_msg(id) {
            m.transport = Transport::Sms;
            m.status = if ok { Status::Sent } else { Status::Failed };
            m.sms_mrs = if srr { mrs } else { Vec::new() };
        }
    }

    fn on_status_report(&mut self, r: sms::StatusReport) {
        for m in self.threads.iter_mut().filter(|t| same_number(&t.number, &r.recipient)).flat_map(|t| t.msgs.iter_mut()) {
            if let Some(p) = m.sms_mrs.iter().position(|&x| x == r.mr) {
                m.sms_mrs.remove(p);
                if m.sms_mrs.is_empty() && r.status < 0x20 && m.status < Status::Delivered {
                    m.status = Status::Delivered;
                }
                return;
            }
        }
    }

    fn on_sms_pdu(&mut self, pdu: &[u8]) {
        let d = match sms::decode(pdu) {
            Some(Tpdu::Deliver(d)) => d,
            Some(Tpdu::Status(r)) => return self.on_status_report(r),
            _ => return,
        };
        let d = match self.reasm.push(d) {
            Some(d) => d,
            None => return,
        };
        if let Some((dst, _)) = d.udh.ports {
            if dst == crate::mms::WAP_PUSH_PORT {
                if let Some(body) = crate::mms::parse_wap_push(&d.data) {
                    self.mms_on_push(&body);
                }
            }
            return;
        }
        let mut m = self.new_msg(true, Transport::Sms, &d.text);
        m.time = d.scts;
        self.push_incoming(&d.from, m);
    }

    /// Called when the user opens a thread: mark read and send RCS display receipts.
    pub fn mark_read(&mut self, number: &str) {
        let t = self.thread_idx(number);
        let mut receipts = Vec::new();
        for m in &mut self.threads[t].msgs {
            if m.unread {
                m.unread = false;
                if m.transport == Transport::Rcs && m.wants_display && !m.imdn_id.is_empty() {
                    receipts.push(m.imdn_id.clone());
                }
            }
        }
        if self.settings.read_receipts {
            for id in receipts {
                self.rcs_send_imdn(number, &id, crate::cpim::Disposition::Displayed);
            }
        }
    }

    pub fn unread_total(&self) -> usize {
        self.threads.iter().map(|t| t.unread()).sum()
    }

    pub fn delete_thread(&mut self, number: &str) {
        self.threads.retain(|t| !same_number(&t.number, number));
    }

    /// Is the peer RCS capable (if known)?
    pub fn peer_rcs(&self, number: &str) -> Option<bool> {
        self.rcs.caps.iter().find(|(n, _)| same_number(n, number)).map(|(_, c)| c.0)
    }

    // ------------------------------------------------------------ sockets

    pub(crate) fn sock_open(&mut self, cid: u8, host: &str, port: u16, owner: Owner) -> Option<u8> {
        if !self.net.pdp_up {
            return None;
        }
        let id = (0u8..12).find(|i| !self.socks.contains_key(i))?;
        let r = self.modem.cmd(&format!("AT+QIOPEN={},{},\"TCP\",\"{}\",{},0,1", cid, id, host, port));
        if !r.ok {
            return None;
        }
        self.socks.insert(id, Sock { owner, connected: false, txq: Vec::new() });
        Some(id)
    }

    pub(crate) fn sock_send(&mut self, id: u8, data: &[u8]) -> bool {
        match self.socks.get_mut(&id) {
            None => false,
            Some(s) if !s.connected => {
                s.txq.extend_from_slice(data);
                true
            }
            Some(_) => {
                for chunk in data.chunks(512) {
                    if !self.modem.cmd(&format!("AT+QISENDEX={},\"{}\"", id, hex(chunk))).ok {
                        return false;
                    }
                }
                true
            }
        }
    }

    pub(crate) fn sock_close(&mut self, id: u8) {
        if self.socks.remove(&id).is_some() {
            self.modem.cmd(&format!("AT+QICLOSE={}", id));
        }
    }

    fn on_sock_open(&mut self, id: u8, ok: bool) {
        let (owner, q) = match self.socks.get_mut(&id) {
            Some(s) => {
                s.connected = ok;
                (s.owner, core::mem::take(&mut s.txq))
            }
            None => return,
        };
        if !ok {
            self.socks.remove(&id);
            self.dispatch_closed(owner, id);
            return;
        }
        if !q.is_empty() {
            self.sock_send(id, &q);
        }
        match owner {
            Owner::Rcs => self.rcs_connected(id),
            Owner::Mms(tx) => self.mms_connected(tx, id),
        }
    }

    fn on_sock_data(&mut self, id: u8, d: &[u8]) {
        match self.socks.get(&id).map(|s| s.owner) {
            Some(Owner::Rcs) => self.rcs_data(id, d),
            Some(Owner::Mms(tx)) => self.mms_data(tx, id, d),
            None => {}
        }
    }

    fn on_sock_closed(&mut self, id: u8) {
        if let Some(s) = self.socks.remove(&id) {
            self.dispatch_closed(s.owner, id);
        }
    }

    fn dispatch_closed(&mut self, owner: Owner, id: u8) {
        match owner {
            Owner::Rcs => self.rcs_closed(id),
            Owner::Mms(tx) => self.mms_closed(tx),
        }
    }

    pub fn simulate(&mut self, n: u32) {
        self.modem.cmd(&format!("AT+SIMEV={}", n));
    }
}
