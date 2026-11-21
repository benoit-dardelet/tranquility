//! Simulated IMS core (P-/S-CSCF + registrar) with an RCS messaging
//! application server: digest-authenticated REGISTER, OPTIONS capability
//! discovery and OMA CPM pager-mode MESSAGE with IMDN / isComposing.
use super::{bots, Ev, Sched, IMS_DOMAIN, IMS_PASSWORD, OWN_NUMBER};
use crate::cpim::{self, CpimOut, Disposition};
use crate::msg::TextMsg;
use crate::prelude::*;
use crate::sip::{self, ICSI_CPM_MSG, IARI_FT_HTTP};
use crate::util::{iso8601, same_number};

#[derive(Clone, Debug)]
pub enum Timed {
    Imdn { from: String, msg_id: String, disp: Disposition },
    Composing { from: String, active: bool },
    Reply { from: String, text: String },
}

#[derive(Default)]
pub struct Ims {
    rx: BTreeMap<u8, Vec<u8>>,
    reg_conn: Option<u8>,
    nonce: String,
    seq: u32,
}

impl Ims {
    pub fn on_close(&mut self, conn: u8) {
        self.rx.remove(&conn);
        if self.reg_conn == Some(conn) {
            self.reg_conn = None;
        }
    }

    pub fn registered(&self) -> bool {
        self.reg_conn.is_some()
    }

    pub fn on_data(&mut self, conn: u8, data: &[u8], s: &mut Sched) {
        let buf = self.rx.entry(conn).or_default();
        buf.extend_from_slice(data);
        loop {
            let buf = self.rx.entry(conn).or_default();
            match TextMsg::parse(buf) {
                Ok(Some((m, n))) => {
                    buf.drain(..n);
                    self.handle(conn, m, s);
                }
                Ok(None) => break,
                Err(()) => {
                    buf.clear();
                    break;
                }
            }
        }
    }

    fn send(&self, conn: u8, m: &TextMsg, delay: u64, s: &mut Sched) {
        s.at(delay, Ev::SockData(conn, m.to_bytes()));
    }

    fn handle(&mut self, conn: u8, m: TextMsg, s: &mut Sched) {
        let method = match m.method() {
            Some(x) => x.to_string(),
            None => return, // responses to our requests
        };
        let target = sip::uri_user(m.uri().unwrap_or(""));
        match method.as_str() {
            "REGISTER" => self.register(conn, &m, s),
            "OPTIONS" => {
                if self.reg_conn != Some(conn) {
                    return self.send(conn, &sip::response_to(&m, 403, "Forbidden", Some("ims")), 40, s);
                }
                let capable = same_number(&target, OWN_NUMBER) || bots::find(&target).is_some_and(|b| b.rcs);
                let r = if capable {
                    let mut r = sip::response_to(&m, 200, "OK", Some("cap"));
                    r.add("Contact", &format!("<sip:{}@{}>;+g.3gpp.icsi-ref=\"{}\";+g.3gpp.iari-ref=\"{}\"", target, IMS_DOMAIN, ICSI_CPM_MSG, IARI_FT_HTTP));
                    r
                } else {
                    sip::response_to(&m, 404, "Not Found", Some("cap"))
                };
                self.send(conn, &r, 250, s);
            }
            "MESSAGE" => {
                if self.reg_conn != Some(conn) {
                    return self.send(conn, &sip::response_to(&m, 403, "Forbidden", Some("ims")), 40, s);
                }
                let bot = bots::find(&target).filter(|b| b.rcs);
                let own = same_number(&target, OWN_NUMBER);
                if bot.is_none() && !own {
                    return self.send(conn, &sip::response_to(&m, 404, "Not Found", Some("as")), 200, s);
                }
                self.send(conn, &sip::response_to(&m, 202, "Accepted", Some("as")), 150, s);
                let body = m.body_str();
                let c = match cpim::decode(&body) {
                    Some(c) => c,
                    None => return,
                };
                if !c.is_text() {
                    return; // IMDN / isComposing from the phone: nothing to do
                }
                let id = c.message_id().unwrap_or("").to_string();
                if own {
                    s.at(800, Ev::Ims(Timed::Reply { from: OWN_NUMBER.into(), text: c.body.clone() }));
                    return;
                }
                let b = bot.unwrap();
                if c.wants("positive-delivery") {
                    s.at(600, Ev::Ims(Timed::Imdn { from: b.number.into(), msg_id: id.clone(), disp: Disposition::Delivered }));
                }
                if c.wants("display") {
                    s.at(1400, Ev::Ims(Timed::Imdn { from: b.number.into(), msg_id: id, disp: Disposition::Displayed }));
                }
                if let Some(reply) = bots::reply(b, &c.body, true) {
                    s.at(1800, Ev::Ims(Timed::Composing { from: b.number.into(), active: true }));
                    s.at(4200, Ev::Ims(Timed::Reply { from: b.number.into(), text: reply }));
                }
            }
            "BYE" | "CANCEL" | "ACK" | "SUBSCRIBE" | "PUBLISH" => {
                if method != "ACK" {
                    self.send(conn, &sip::response_to(&m, 200, "OK", Some("ims")), 40, s);
                }
            }
            _ => self.send(conn, &sip::response_to(&m, 405, "Method Not Allowed", Some("ims")), 40, s),
        }
    }

    fn register(&mut self, conn: u8, m: &TextMsg, s: &mut Sched) {
        let auth = m.header("Authorization").map(|a| a.to_string());
        let ok = auth.as_deref().is_some_and(|a| a.contains(&self.nonce) && !self.nonce.is_empty() && sip::verify(a, IMS_PASSWORD, "REGISTER"));
        if !ok {
            self.nonce = format!("{:016x}", s.rng.next_u64());
            let mut r = sip::response_to(m, 401, "Unauthorized", Some("reg"));
            r.add("WWW-Authenticate", &format!("Digest realm=\"{}\", nonce=\"{}\", algorithm=MD5, qop=\"auth\"", IMS_DOMAIN, self.nonce));
            return self.send(conn, &r, 120, s);
        }
        let expires: u32 = m.header("Expires").and_then(|e| e.parse().ok()).unwrap_or(600);
        let mut r = sip::response_to(m, 200, "OK", Some("reg"));
        if expires == 0 {
            self.reg_conn = None;
        } else {
            self.reg_conn = Some(conn);
            if let Some(c) = m.header("Contact") {
                r.add("Contact", &format!("{};expires={}", c, expires));
            }
            r.add("P-Associated-URI", &format!("<sip:{}@{}>, <tel:{}>", OWN_NUMBER, IMS_DOMAIN, OWN_NUMBER));
            r.add("Service-Route", &format!("<sip:orig@scscf.{};lr>", IMS_DOMAIN));
        }
        r.add("Expires", &expires.to_string());
        self.send(conn, &r, 150, s);
    }

    fn push_message(&mut self, from: &str, content_type: &str, body: &str, disposition: Option<&str>, s: &mut Sched) -> bool {
        let conn = match self.reg_conn {
            Some(c) => c,
            None => return false,
        };
        self.seq += 1;
        let msg_id = format!("as{}{:08x}", self.seq, s.rng.next_u64() as u32);
        let cp = cpim::encode(&CpimOut {
            from: &format!("sip:{}@{}", from, IMS_DOMAIN),
            to: &format!("sip:{}@{}", OWN_NUMBER, IMS_DOMAIN),
            msg_id: &msg_id,
            datetime: &iso8601(s.unix()),
            disposition,
            content_type,
            body,
        });
        let m = TextMsg::request("MESSAGE", &format!("sip:{}@{}", OWN_NUMBER, IMS_DOMAIN), "SIP/2.0")
            .h("Via", &format!("SIP/2.0/TCP scscf.{};branch=z9hG4bKas{}", IMS_DOMAIN, self.seq))
            .h("Max-Forwards", "68")
            .h("From", &format!("<sip:{}@{}>;tag=as{}", from, IMS_DOMAIN, self.seq))
            .h("To", &format!("<sip:{}@{}>", OWN_NUMBER, IMS_DOMAIN))
            .h("Call-ID", &format!("as-{}-{:x}@{}", self.seq, s.rng.next_u64(), IMS_DOMAIN))
            .h("CSeq", "1 MESSAGE")
            .h("P-Asserted-Identity", &format!("<tel:{}>", from))
            .h("Accept-Contact", &format!("*;+g.3gpp.icsi-ref=\"{}\"", ICSI_CPM_MSG))
            .body(cpim::CT_CPIM, cp.as_bytes());
        self.send(conn, &m, 0, s);
        true
    }

    pub fn timed(&mut self, t: Timed, s: &mut Sched) {
        match t {
            Timed::Imdn { from, msg_id, disp } => {
                let xml = cpim::imdn_xml(&msg_id, &iso8601(s.unix()), disp);
                self.push_message(&from, cpim::CT_IMDN, &xml, None, s);
            }
            Timed::Composing { from, active } => {
                self.push_message(&from, cpim::CT_COMPOSING, &cpim::iscomposing_xml(active), None, s);
            }
            Timed::Reply { from, text } => {
                if !self.push_message(&from, cpim::CT_TEXT, &text, Some("positive-delivery, display"), s) {
                    s.at(0, Ev::SmsFrom { from, text });
                }
            }
        }
    }

    /// Bot-initiated chat (simulator control).
    pub fn inject(&mut self, from: &str, text: &str, s: &mut Sched) {
        s.at(300, Ev::Ims(Timed::Composing { from: from.into(), active: true }));
        s.at(2500, Ev::Ims(Timed::Reply { from: from.into(), text: text.into() }));
    }
}
