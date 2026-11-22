//! RCS client (GSMA RCS Universal Profile, pager-mode CPM messaging):
//! IMS registration with digest auth, OPTIONS capability discovery,
//! SIP MESSAGE with CPIM, IMDN delivery/display receipts, isComposing,
//! and transparent fallback to SMS.
use super::*;
use super::ft::CT_FT;
use crate::cpim::{self, CpimOut, Disposition};
use crate::mime;
use crate::msg::TextMsg;
use crate::sip::{self, Challenge, IARI_FT_HTTP, ICSI_CPM_MSG};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RegState {
    #[default]
    Off,
    Connecting,
    Registering,
    Registered,
    Failed,
}

impl RegState {
    pub fn label(self) -> &'static str {
        match self {
            RegState::Off => "Disconnected",
            RegState::Connecting => "Connecting",
            RegState::Registering => "Registering",
            RegState::Registered => "Connected",
            RegState::Failed => "Error, retrying",
        }
    }
}

#[derive(Debug)]
enum TxKind {
    Register,
    Options(String),
    Message(u32),
    Other,
}

#[derive(Debug)]
struct Tx {
    call_id: String,
    kind: TxKind,
    sent: u64,
}

#[derive(Default)]
pub struct Rcs {
    pub state: RegState,
    conn: Option<u8>,
    rx: Vec<u8>,
    cseq: u32,
    reg_call_id: String,
    tag: String,
    challenge: Option<Challenge>,
    auth_tries: u8,
    nc: u32,
    /// number -> (capable, checked at)
    pub caps: BTreeMap<String, (bool, u64)>,
    txs: Vec<Tx>,
    outbox: Vec<(u32, String)>,
    refresh_at: u64,
    retry_at: u64,
    composing_sent: BTreeMap<String, u64>,
    pub expires: u32,
}

const CAP_TTL: u64 = 10 * 60 * 1000;
const LOCAL_IP: &str = "10.64.0.4";
pub const CONF_FACTORY: &str = "conf-factory";

impl Core {
    fn me(&self) -> String {
        format!("sip:{}@{}", self.info.number, IMS_HOST)
    }

    fn impi(&self) -> String {
        format!("{}@{}", self.info.imsi, IMS_HOST)
    }

    pub fn rcs_usable(&self) -> bool {
        self.settings.rcs && self.rcs.state == RegState::Registered
    }

    pub(crate) fn rcs_start(&mut self) {
        if !self.settings.rcs || !self.net.registered || !self.net.pdp_up || self.rcs.conn.is_some() {
            return;
        }
        if !matches!(self.rcs.state, RegState::Off | RegState::Failed) {
            return;
        }
        match self.sock_open(CID_IMS, IMS_HOST, 5060, Owner::Rcs) {
            Some(id) => {
                self.rcs.conn = Some(id);
                self.rcs.state = RegState::Connecting;
            }
            None => {
                self.rcs.state = RegState::Failed;
                self.rcs.retry_at = self.now + 15_000;
            }
        }
    }

    pub(crate) fn rcs_stop(&mut self) {
        if self.rcs.state == RegState::Registered {
            self.send_register(0);
        }
        if let Some(c) = self.rcs.conn {
            self.sock_close(c);
        }
        self.rcs_reset();
        self.rcs.state = RegState::Off;
    }

    /// Drop all RCS state; outstanding chat messages fall back to SMS.
    pub(crate) fn rcs_reset(&mut self) {
        self.rcs.conn = None;
        self.rcs.rx.clear();
        self.rcs.challenge = None;
        self.rcs.state = RegState::Off;
        let txs = core::mem::take(&mut self.rcs.txs);
        for t in txs {
            if let TxKind::Message(id) = t.kind {
                self.rcs_fallback(id);
            }
        }
        let ob = core::mem::take(&mut self.rcs.outbox);
        for (id, _) in ob {
            self.rcs_fallback(id);
        }
    }

    pub(crate) fn rcs_connected(&mut self, id: u8) {
        self.rcs.conn = Some(id);
        self.rcs.state = RegState::Registering;
        self.rcs.reg_call_id = format!("{}@{}", self.rng.token(16), LOCAL_IP);
        self.rcs.tag = self.rng.token(8);
        self.rcs.auth_tries = 0;
        self.rcs.challenge = None;
        self.send_register(600);
    }

    pub(crate) fn rcs_closed(&mut self, id: u8) {
        if self.rcs.conn == Some(id) {
            let was = self.rcs.state;
            self.rcs_reset();
            if self.settings.rcs {
                self.rcs.state = RegState::Failed;
                self.rcs.retry_at = self.now + 5_000;
            }
            if was == RegState::Registered {
                self.toast("Chat features disconnected");
            }
        }
    }

    fn sip_send(&mut self, m: &TextMsg) {
        if let Some(c) = self.rcs.conn {
            let b = m.to_bytes();
            self.sock_send(c, &b);
        }
    }

    fn base_request(&mut self, method: &str, uri: &str, to: &str, call_id: &str, cseq: u32, tag: &str) -> TextMsg {
        let branch = self.rng.token(12);
        TextMsg::request(method, uri, "SIP/2.0")
            .h("Via", &format!("SIP/2.0/TCP {}:5060;branch=z9hG4bK{};rport", LOCAL_IP, branch))
            .h("Max-Forwards", "70")
            .h("From", &format!("<{}>;tag={}", self.me(), tag))
            .h("To", &format!("<{}>", to))
            .h("Call-ID", call_id)
            .h("CSeq", &format!("{} {}", cseq, method))
            .h("User-Agent", "Tranquility/0.1 RCS-UP/2.4")
    }

    fn send_register(&mut self, expires: u32) {
        self.rcs.cseq += 1;
        let me = self.me();
        let uri = format!("sip:{}", IMS_HOST);
        let (cid, cseq, tag) = (self.rcs.reg_call_id.clone(), self.rcs.cseq, self.rcs.tag.clone());
        let mut m = self.base_request("REGISTER", &uri, &me, &cid, cseq, &tag);
        m.add(
            "Contact",
            &format!(
                "<sip:{}@{}:5060;transport=tcp>;+sip.instance=\"<urn:gsma:imei:{}>\";+g.3gpp.icsi-ref=\"{}\";+g.3gpp.iari-ref=\"{}\"",
                self.info.number, LOCAL_IP, self.info.imei, ICSI_CPM_MSG, IARI_FT_HTTP
            ),
        );
        m.add("Expires", &expires.to_string());
        m.add("Supported", "path, gruu");
        if let Some(ch) = self.rcs.challenge.clone() {
            self.rcs.nc += 1;
            let cnonce = self.rng.token(10);
            let pass = self.settings.ims_password.clone();
            m.add("Authorization", &sip::authorization(&ch, &self.impi(), &pass, "REGISTER", &uri, self.rcs.nc, &cnonce));
        }
        self.rcs.txs.retain(|t| !matches!(t.kind, TxKind::Register));
        self.rcs.txs.push(Tx { call_id: cid, kind: TxKind::Register, sent: self.now });
        self.sip_send(&m);
    }

    fn new_dialogless(&mut self, method: &str, to_number: &str, kind: TxKind) -> TextMsg {
        let uri = format!("sip:{}@{}", to_number, IMS_HOST);
        let call_id = format!("{}@{}", self.rng.token(16), LOCAL_IP);
        let tag = self.rng.token(8);
        self.rcs.txs.push(Tx { call_id: call_id.clone(), kind, sent: self.now });
        let mut m = self.base_request(method, &uri, &uri, &call_id, 1, &tag);
        m.add("P-Preferred-Identity", &format!("<{}>", self.me()));
        m.add("Accept-Contact", &format!("*;+g.3gpp.icsi-ref=\"{}\"", ICSI_CPM_MSG));
        m
    }

    pub(crate) fn rcs_send(&mut self, id: u32, to: &str, _text: &str) {
        let cap = self.rcs.caps.iter().find(|(n, _)| same_number(n, to)).map(|(_, c)| *c);
        match cap {
            Some((true, t)) if self.now.saturating_sub(t) < CAP_TTL => self.send_cpm(id, to),
            Some((false, t)) if self.now.saturating_sub(t) < CAP_TTL => self.rcs_fallback(id),
            _ => {
                self.rcs.outbox.push((id, to.to_string()));
                self.rcs_query_caps(to);
            }
        }
    }

    /// Capability discovery via SIP OPTIONS.
    pub fn rcs_query_caps(&mut self, to: &str) {
        if !self.rcs_usable() {
            return;
        }
        if self.rcs.txs.iter().any(|t| matches!(&t.kind, TxKind::Options(n) if same_number(n, to))) {
            return;
        }
        let mut m = self.new_dialogless("OPTIONS", to, TxKind::Options(to.to_string()));
        m.add("Contact", &format!("<sip:{}@{}:5060;transport=tcp>;+g.3gpp.icsi-ref=\"{}\"", self.info.number, LOCAL_IP, ICSI_CPM_MSG));
        m.add("Accept", "application/sdp");
        self.sip_send(&m);
    }

    fn cpim_body(&self, msg_id: &str, disposition: Option<&str>, ctype: &str, body: &str) -> String {
        cpim::encode(&CpimOut {
            from: "sip:anonymous@anonymous.invalid",
            to: "sip:anonymous@anonymous.invalid",
            msg_id,
            datetime: &util::iso8601(self.unix()),
            disposition,
            content_type: ctype,
            body,
        })
    }

    fn cpim_message(&mut self, to: &str, kind: TxKind, msg_id: &str, disposition: Option<&str>, ctype: &str, body: &str) {
        let cp = cpim::encode(&CpimOut {
            from: "sip:anonymous@anonymous.invalid",
            to: "sip:anonymous@anonymous.invalid",
            msg_id,
            datetime: &util::iso8601(self.unix()),
            disposition,
            content_type: ctype,
            body,
        });
        let m = self.new_dialogless("MESSAGE", to, kind).body(cpim::CT_CPIM, cp.as_bytes());
        self.sip_send(&m);
    }

    fn send_cpm(&mut self, id: u32, to: &str) {
        let (text, files) = match self.find_msg(id) {
            Some(m) => {
                m.transport = Transport::Rcs;
                (m.text.clone(), !m.attachments.is_empty())
            }
            None => return,
        };
        self.rcs.composing_sent.remove(to);
        if !text.is_empty() {
            self.rcs_payload(id, to, cpim::CT_TEXT, &text);
        }
        if files {
            self.ft_upload(id, 0);
        }
    }

    /// IMDN id for a payload of message `id` (the first payload owns the receipts).
    fn payload_imdn(&mut self, id: u32) -> String {
        let fresh = self.rng.token(16);
        match self.find_msg(id) {
            Some(m) if m.imdn_id.is_empty() => {
                m.imdn_id = fresh.clone();
                fresh
            }
            _ => fresh,
        }
    }

    fn disposition(&self) -> &'static str {
        if self.settings.delivery_reports {
            "positive-delivery, display"
        } else {
            "display"
        }
    }

    /// One CPIM payload (text or file-info) to a single peer.
    pub(crate) fn rcs_payload(&mut self, id: u32, to: &str, ctype: &str, body: &str) {
        let imdn = self.payload_imdn(id);
        let disp = self.disposition();
        self.cpim_message(to, TxKind::Message(id), &imdn, Some(disp), ctype, body);
    }

    pub(crate) fn rcs_send_group(&mut self, id: u32, key: &str) {
        let (text, files) = match self.find_msg(id) {
            Some(m) => (m.text.clone(), !m.attachments.is_empty()),
            None => return,
        };
        if !text.is_empty() {
            self.rcs_group_payload(id, key, cpim::CT_TEXT, &text);
        }
        if files {
            self.ft_upload(id, 0);
        }
    }

    /// Pager-mode group message: a multi-recipient MESSAGE (RFC 5365) to the
    /// conference factory, carrying the recipient list and the CPIM payload.
    pub(crate) fn rcs_group_payload(&mut self, id: u32, key: &str, ctype: &str, body: &str) {
        let (members, name, conv) = match self.thread(key) {
            Some(t) => (t.members.clone(), t.group_name.clone(), t.group_id().to_string()),
            None => return,
        };
        let imdn = self.payload_imdn(id);
        let disp = self.disposition();
        let cp = self.cpim_body(&imdn, Some(disp), ctype, body);
        let uris: Vec<String> = members.iter().map(|m| format!("sip:{}@{}", m, IMS_HOST)).collect();
        let boundary = format!("tqgrp{}", self.rng.token(10));
        let (ct, mbody) = mime::encode(
            "mixed",
            &boundary,
            &[
                mime::Part::new(mime::CT_RESOURCE_LISTS, mime::resource_list(&uris).as_bytes()).h("Content-Disposition", "recipient-list"),
                mime::Part::new(cpim::CT_CPIM, cp.as_bytes()),
            ],
        );
        let contribution = self.rng.token(16);
        let mut m = self.new_dialogless("MESSAGE", CONF_FACTORY, TxKind::Message(id));
        m.add("Require", "recipient-list-message");
        m.add("Conversation-ID", &conv);
        m.add("Contribution-ID", &contribution);
        m.add("Subject", &name);
        let m = m.body(&ct, &mbody);
        self.sip_send(&m);
    }

    pub(crate) fn rcs_send_imdn(&mut self, to: &str, msg_id: &str, d: Disposition) {
        if !self.rcs_usable() {
            return;
        }
        let xml = cpim::imdn_xml(msg_id, &util::iso8601(self.unix()), d);
        let own = self.rng.token(16);
        self.cpim_message(to, TxKind::Other, &own, None, cpim::CT_IMDN, &xml);
    }

    /// Typing notification; throttled, and only towards RCS-capable peers.
    pub fn rcs_composing(&mut self, to: &str, active: bool) {
        if to.starts_with(GROUP_PREFIX) {
            return; // pager-mode groups don't carry isComposing
        }
        if !self.rcs_usable() || !self.settings.typing_indicators || self.peer_rcs(to) != Some(true) {
            return;
        }
        let last = self.rcs.composing_sent.get(to).copied();
        if active && last.is_some_and(|t| self.now.saturating_sub(t) < 5_000) {
            return;
        }
        if !active && last.is_none() {
            return;
        }
        if active {
            self.rcs.composing_sent.insert(to.to_string(), self.now);
        } else {
            self.rcs.composing_sent.remove(to);
        }
        let own = self.rng.token(16);
        self.cpim_message(to, TxKind::Other, &own, None, cpim::CT_COMPOSING, &cpim::iscomposing_xml(active));
    }

    /// Chat unavailable: SMS for 1:1 text, MMS for pictures and groups.
    pub(crate) fn rcs_fallback(&mut self, id: u32) {
        let (to, members, group, m) = match self.threads.iter().find(|t| t.msgs.iter().any(|m| m.id == id)) {
            Some(t) => (t.number.clone(), t.members.clone(), t.is_group(), t.msgs.iter().find(|m| m.id == id).cloned()),
            None => return,
        };
        let m = match m {
            Some(m) => m,
            None => return,
        };
        if m.status >= Status::Sent && m.transport != Transport::Rcs {
            return;
        }
        if group {
            self.mms_send(id, &members, &m.text, m.attachments);
        } else if !m.attachments.is_empty() {
            self.mms_send(id, &[to], &m.text, m.attachments);
        } else {
            self.send_sms(id, &to, &m.text);
        }
    }

    fn flush_outbox(&mut self, number: &str) {
        let (mine, rest): (Vec<_>, Vec<_>) = core::mem::take(&mut self.rcs.outbox).into_iter().partition(|(_, n)| same_number(n, number));
        self.rcs.outbox = rest;
        let capable = self.peer_rcs(number) == Some(true) && self.rcs_usable();
        for (id, to) in mine {
            if capable {
                self.send_cpm(id, &to);
            } else {
                self.rcs_fallback(id);
            }
        }
    }

    pub(crate) fn rcs_data(&mut self, _id: u8, d: &[u8]) {
        self.rcs.rx.extend_from_slice(d);
        loop {
            match TextMsg::parse(&self.rcs.rx) {
                Ok(Some((m, n))) => {
                    self.rcs.rx.drain(..n);
                    if m.is_response() {
                        self.sip_response(m);
                    } else {
                        self.sip_request(m);
                    }
                }
                Ok(None) => break,
                Err(()) => {
                    self.rcs.rx.clear();
                    break;
                }
            }
        }
    }

    fn sip_response(&mut self, m: TextMsg) {
        let cid = m.header("Call-ID").unwrap_or("").to_string();
        let pos = match self.rcs.txs.iter().position(|t| t.call_id == cid) {
            Some(p) => p,
            None => return,
        };
        let code = m.status().unwrap_or(500);
        if code < 200 {
            return; // provisional
        }
        let tx = self.rcs.txs.remove(pos);
        match tx.kind {
            TxKind::Register => {
                if code == 401 || code == 407 {
                    let h = m.header("WWW-Authenticate").or_else(|| m.header("Proxy-Authenticate")).unwrap_or("");
                    self.rcs.auth_tries += 1;
                    match sip::parse_challenge(h) {
                        Some(ch) if self.rcs.auth_tries <= 2 => {
                            self.rcs.challenge = Some(ch);
                            self.rcs.nc = 0;
                            self.send_register(600);
                        }
                        _ => self.rcs_fail("Chat: authentication failed"),
                    }
                } else if (200..300).contains(&code) {
                    let exp: u32 = m.header("Expires").and_then(|e| e.parse().ok()).unwrap_or(600);
                    if exp == 0 {
                        self.rcs.state = RegState::Off;
                    } else {
                        let first = self.rcs.state != RegState::Registered;
                        self.rcs.state = RegState::Registered;
                        self.rcs.expires = exp;
                        self.rcs.refresh_at = self.now + exp as u64 * 500;
                        if first {
                            self.toast("Chat features connected");
                        }
                    }
                } else {
                    self.rcs_fail("Chat: registration rejected");
                }
            }
            TxKind::Options(number) => {
                let capable = (200..300).contains(&code) && m.headers_all("Contact").iter().any(|c| c.contains(ICSI_CPM_MSG));
                self.rcs.caps.insert(number.clone(), (capable, self.now));
                self.flush_outbox(&number);
            }
            TxKind::Message(id) => {
                if (200..300).contains(&code) {
                    if let Some(msg) = self.find_msg(id) {
                        if msg.status < Status::Sent {
                            msg.status = Status::Sent;
                        }
                    }
                } else {
                    if let Some(t) = self.threads.iter().find(|t| t.msgs.iter().any(|x| x.id == id)) {
                        let n = t.number.clone();
                        self.rcs.caps.insert(n, (false, self.now));
                    }
                    self.rcs_fallback(id);
                }
            }
            TxKind::Other => {}
        }
    }

    fn rcs_fail(&mut self, why: &str) {
        self.toast(why);
        if let Some(c) = self.rcs.conn {
            self.sock_close(c);
        }
        self.rcs_reset();
        self.rcs.state = RegState::Failed;
        self.rcs.retry_at = self.now + 60_000;
    }

    fn sip_request(&mut self, m: TextMsg) {
        let method = m.method().unwrap_or("").to_string();
        match method.as_str() {
            "MESSAGE" => {
                let r = sip::response_to(&m, 200, "OK", Some(&self.rng.token(8)));
                self.sip_send(&r);
                let from = sip::uri_user(m.header("P-Asserted-Identity").or_else(|| m.header("From")).unwrap_or(""));
                let ctype = m.header("Content-Type").unwrap_or("").to_string();
                let mut key = from.clone();
                let mut cpim_body = m.body_str();
                if ctype.to_ascii_lowercase().starts_with("multipart/") {
                    // Group message: recipient list + CPIM payload.
                    let parts = mime::decode(&ctype, &m.body);
                    let mut members: Vec<String> = Vec::new();
                    cpim_body.clear();
                    for p in &parts {
                        let pct = p.content_type().to_ascii_lowercase();
                        if pct.starts_with(mime::CT_RESOURCE_LISTS) {
                            members = mime::parse_resource_list(&String::from_utf8_lossy(&p.body)).iter().map(|u| sip::uri_user(u)).collect();
                        } else if pct.starts_with(cpim::CT_CPIM) {
                            cpim_body = String::from_utf8_lossy(&p.body).into_owned();
                        }
                    }
                    if let Some(conv) = m.header("Conversation-ID").map(|c| c.to_string()) {
                        members.push(from.clone());
                        let name = m.header("Subject").unwrap_or("").to_string();
                        key = self.create_group(&name, &members, Some(&conv));
                    }
                }
                let c = match cpim::decode(&cpim_body) {
                    Some(c) => c,
                    None => return,
                };
                let ct = c.content_type.to_ascii_lowercase();
                self.rcs.caps.insert(from.clone(), (true, self.now));
                let imdn = c.message_id().unwrap_or("").to_string();
                if c.is_text() || ct.starts_with(CT_FT) {
                    if c.is_text() {
                        let mut msg = self.new_msg(true, Transport::Rcs, &c.body);
                        msg.imdn_id = imdn.clone();
                        msg.wants_display = c.wants("display");
                        msg.from = from.clone();
                        self.push_incoming(&key, msg);
                    } else {
                        self.ft_incoming(&key, &from, &imdn, c.wants("display"), &c.body);
                    }
                    if c.wants("positive-delivery") && !imdn.is_empty() {
                        self.rcs_send_imdn(&from, &imdn, Disposition::Delivered);
                    }
                } else if ct.starts_with(cpim::CT_IMDN) {
                    if let Some((id, d)) = cpim::parse_imdn(&c.body) {
                        let new = if d == Disposition::Displayed { Status::Read } else { Status::Delivered };
                        if let Some(msg) = self.threads.iter_mut().flat_map(|t| t.msgs.iter_mut()).find(|x| !x.incoming && x.imdn_id == id) {
                            if msg.status < new {
                                msg.status = new;
                            }
                        }
                    }
                } else if ct.starts_with(cpim::CT_COMPOSING) {
                    if let Some(active) = cpim::parse_iscomposing(&c.body) {
                        let t = self.thread_idx(&key);
                        self.threads[t].typing_until = if active { self.now + 15_000 } else { 0 };
                        self.threads[t].typing_who = from.clone();
                    }
                }
            }
            "OPTIONS" => {
                let mut r = sip::response_to(&m, 200, "OK", Some(&self.rng.token(8)));
                r.add("Contact", &format!("<sip:{}@{}:5060;transport=tcp>;+g.3gpp.icsi-ref=\"{}\"", self.info.number, LOCAL_IP, ICSI_CPM_MSG));
                self.sip_send(&r);
            }
            "ACK" => {}
            _ => {
                let r = sip::response_to(&m, 405, "Method Not Allowed", Some(&self.rng.token(8)));
                self.sip_send(&r);
            }
        }
    }

    pub(crate) fn rcs_tick(&mut self) {
        let now = self.now;
        let (expired, keep): (Vec<Tx>, Vec<Tx>) = core::mem::take(&mut self.rcs.txs).into_iter().partition(|t| now.saturating_sub(t.sent) > 8_000);
        self.rcs.txs = keep;
        for t in expired {
            match t.kind {
                TxKind::Register => self.rcs_fail("Chat: registration timed out"),
                TxKind::Options(n) => {
                    self.rcs.caps.insert(n.clone(), (false, now));
                    self.flush_outbox(&n);
                }
                TxKind::Message(id) => self.rcs_fallback(id),
                TxKind::Other => {}
            }
        }
        match self.rcs.state {
            RegState::Registered if now >= self.rcs.refresh_at => {
                self.rcs.refresh_at = now + self.rcs.expires as u64 * 500;
                self.send_register(self.rcs.expires);
            }
            RegState::Failed | RegState::Off if self.settings.rcs && now >= self.rcs.retry_at && self.net.pdp_up && self.rcs.conn.is_none() => {
                self.rcs.retry_at = now + 30_000;
                self.rcs_start();
            }
            _ => {}
        }
    }
}
