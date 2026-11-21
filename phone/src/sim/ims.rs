//! Simulated IMS core (P-/S-CSCF + registrar) with an RCS messaging
//! application server: digest-authenticated REGISTER, OPTIONS capability
//! discovery, OMA CPM pager-mode MESSAGE with IMDN / isComposing, a
//! conference factory that fans out RFC 5365 multi-recipient group
//! messages, and file-transfer-over-HTTP file-info relaying.
use super::ftcs::Ftcs;
use super::{bots, Ev, Sched, IMS_DOMAIN, IMS_PASSWORD, OWN_NUMBER};
use crate::cpim::{self, CpimOut, Disposition};
use crate::mime;
use crate::msg::TextMsg;
use crate::prelude::*;
use crate::services::ft::CT_FT;
use crate::sip::{self, IARI_FT_HTTP, ICSI_CPM_MSG};
use crate::util::{iso8601, same_number};

#[derive(Clone, Debug)]
pub struct Group {
    pub conv: String,
    pub name: String,
    /// every participant, including the phone
    pub members: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Timed {
    Imdn { from: String, msg_id: String, disp: Disposition },
    Composing { from: String, active: bool },
    Reply { from: String, text: String, group: Option<Group> },
    /// payload relayed verbatim (e.g. a file-info sent to our own number)
    Raw { from: String, content_type: String, body: String },
    ReplyFile { from: String, group: Option<Group> },
}

#[derive(Default)]
pub struct Ims {
    rx: BTreeMap<u8, Vec<u8>>,
    reg_conn: Option<u8>,
    nonce: String,
    seq: u32,
}

fn rcs_bot(n: &str) -> Option<&'static bots::Bot> {
    bots::find(n).filter(|b| b.rcs)
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
        self.rx.entry(conn).or_default().extend_from_slice(data);
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
                let capable = same_number(&target, OWN_NUMBER) || rcs_bot(&target).is_some();
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
                if target == crate::services::rcs::CONF_FACTORY {
                    return self.group_message(conn, &m, s);
                }
                let bot = rcs_bot(&target);
                let own = same_number(&target, OWN_NUMBER);
                if bot.is_none() && !own {
                    return self.send(conn, &sip::response_to(&m, 404, "Not Found", Some("as")), 200, s);
                }
                self.send(conn, &sip::response_to(&m, 202, "Accepted", Some("as")), 150, s);
                let c = match cpim::decode(&m.body_str()) {
                    Some(c) => c,
                    None => return,
                };
                let is_file = c.content_type.to_ascii_lowercase().starts_with(CT_FT);
                if !c.is_text() && !is_file {
                    return; // IMDN / isComposing from the phone: nothing to do
                }
                if own {
                    let (ct, body) = (c.content_type.clone(), c.body.clone());
                    return s.at(800, Ev::Ims(Timed::Raw { from: OWN_NUMBER.into(), content_type: ct, body }));
                }
                let b = bot.unwrap();
                self.receipts(b.number, &c, s);
                if is_file {
                    s.at(2200, Ev::Ims(Timed::Reply { from: b.number.into(), text: format!("{}: nice picture! Here's one back.", b.name), group: None }));
                    s.at(3600, Ev::Ims(Timed::ReplyFile { from: b.number.into(), group: None }));
                } else if let Some(reply) = bots::reply(b, &c.body, true) {
                    s.at(1800, Ev::Ims(Timed::Composing { from: b.number.into(), active: true }));
                    s.at(4200, Ev::Ims(Timed::Reply { from: b.number.into(), text: reply, group: None }));
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

    fn receipts(&self, from: &str, c: &cpim::Cpim, s: &mut Sched) {
        let id = c.message_id().unwrap_or("").to_string();
        if c.wants("positive-delivery") {
            s.at(600, Ev::Ims(Timed::Imdn { from: from.into(), msg_id: id.clone(), disp: Disposition::Delivered }));
        }
        if c.wants("display") {
            s.at(1400, Ev::Ims(Timed::Imdn { from: from.into(), msg_id: id, disp: Disposition::Displayed }));
        }
    }

    /// Conference factory: explode a multi-recipient MESSAGE to the group.
    fn group_message(&mut self, conn: u8, m: &TextMsg, s: &mut Sched) {
        let ct = m.header("Content-Type").unwrap_or("").to_string();
        let parts = mime::decode(&ct, &m.body);
        let mut recipients = Vec::new();
        let mut cp = None;
        for p in &parts {
            let pct = p.content_type().to_ascii_lowercase();
            if pct.starts_with(mime::CT_RESOURCE_LISTS) {
                recipients = mime::parse_resource_list(&String::from_utf8_lossy(&p.body)).iter().map(|u| sip::uri_user(u)).collect();
            } else if pct.starts_with(cpim::CT_CPIM) {
                cp = cpim::decode(&String::from_utf8_lossy(&p.body));
            }
        }
        let (c, conv) = match (cp, m.header("Conversation-ID")) {
            (Some(c), Some(conv)) if !recipients.is_empty() => (c, conv.to_string()),
            _ => return self.send(conn, &sip::response_to(m, 400, "Bad Request", Some("conf")), 100, s),
        };
        if recipients.iter().any(|r| rcs_bot(r).is_none()) {
            return self.send(conn, &sip::response_to(m, 404, "Not Found", Some("conf")), 150, s);
        }
        self.send(conn, &sip::response_to(m, 202, "Accepted", Some("conf")), 150, s);
        let mut members = recipients.clone();
        members.push(OWN_NUMBER.into());
        let g = Group { conv, name: m.header("Subject").unwrap_or("Group").to_string(), members };
        let is_file = c.content_type.to_ascii_lowercase().starts_with(CT_FT);
        for (i, r) in recipients.iter().enumerate() {
            let b = match rcs_bot(r) {
                Some(b) => b,
                None => continue,
            };
            if i == 0 {
                self.receipts(b.number, &c, s);
            }
            if i < 2 {
                let text = if is_file { format!("{} likes the picture.", b.name) } else { bots::reply(b, &c.body, true).unwrap_or_default() };
                s.at(2500 + 2000 * i as u64, Ev::Ims(Timed::Reply { from: b.number.into(), text, group: Some(g.clone()) }));
            }
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

    fn push_message(&mut self, from: &str, content_type: &str, body: &str, disposition: Option<&str>, group: Option<&Group>, s: &mut Sched) -> bool {
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
        let mut m = TextMsg::request("MESSAGE", &format!("sip:{}@{}", OWN_NUMBER, IMS_DOMAIN), "SIP/2.0")
            .h("Via", &format!("SIP/2.0/TCP scscf.{};branch=z9hG4bKas{}", IMS_DOMAIN, self.seq))
            .h("Max-Forwards", "68")
            .h("From", &format!("<sip:{}@{}>;tag=as{}", from, IMS_DOMAIN, self.seq))
            .h("To", &format!("<sip:{}@{}>", OWN_NUMBER, IMS_DOMAIN))
            .h("Call-ID", &format!("as-{}-{:x}@{}", self.seq, s.rng.next_u64(), IMS_DOMAIN))
            .h("CSeq", "1 MESSAGE")
            .h("P-Asserted-Identity", &format!("<tel:{}>", from))
            .h("Accept-Contact", &format!("*;+g.3gpp.icsi-ref=\"{}\"", ICSI_CPM_MSG));
        match group {
            Some(g) => {
                let uris: Vec<String> = g.members.iter().map(|n| format!("sip:{}@{}", n, IMS_DOMAIN)).collect();
                let (ct, b) = mime::encode(
                    "mixed",
                    &format!("asb{}", self.seq),
                    &[
                        mime::Part::new(mime::CT_RESOURCE_LISTS, mime::resource_list(&uris).as_bytes()).h("Content-Disposition", "recipient-list-history"),
                        mime::Part::new(cpim::CT_CPIM, cp.as_bytes()),
                    ],
                );
                m.add("Conversation-ID", &g.conv);
                m.add("Subject", &g.name);
                m = m.body(&ct, &b);
            }
            None => m = m.body(cpim::CT_CPIM, cp.as_bytes()),
        }
        self.send(conn, &m, 0, s);
        true
    }

    pub fn timed(&mut self, t: Timed, s: &mut Sched, ftcs: &mut Ftcs) {
        const DISP: Option<&str> = Some("positive-delivery, display");
        match t {
            Timed::Imdn { from, msg_id, disp } => {
                let xml = cpim::imdn_xml(&msg_id, &iso8601(s.unix()), disp);
                self.push_message(&from, cpim::CT_IMDN, &xml, None, None, s);
            }
            Timed::Composing { from, active } => {
                self.push_message(&from, cpim::CT_COMPOSING, &cpim::iscomposing_xml(active), None, None, s);
            }
            Timed::Reply { from, text, group } => {
                if !self.push_message(&from, cpim::CT_TEXT, &text, DISP, group.as_ref(), s) && group.is_none() {
                    s.at(0, Ev::SmsFrom { from, text });
                }
            }
            Timed::Raw { from, content_type, body } => {
                self.push_message(&from, &content_type, &body, DISP, None, s);
            }
            Timed::ReplyFile { from, group } => {
                let xml = ftcs.bot_picture(s.unix());
                self.push_message(&from, CT_FT, &xml, DISP, group.as_ref(), s);
            }
        }
    }

    /// Bot-initiated chat (simulator control).
    pub fn inject(&mut self, from: &str, text: &str, s: &mut Sched) {
        s.at(300, Ev::Ims(Timed::Composing { from: from.into(), active: true }));
        s.at(2500, Ev::Ims(Timed::Reply { from: from.into(), text: text.into(), group: None }));
    }

    /// Alice starts a group chat with the phone and Carol.
    pub fn inject_group(&mut self, s: &mut Sched) {
        let g = Group { conv: "sim-weekend-0001".into(), name: "Weekend plans".into(), members: vec!["+15550101".into(), "+15550103".into(), OWN_NUMBER.into()] };
        s.at(500, Ev::Ims(Timed::Reply { from: "+15550101".into(), text: "Hiking on Saturday? Carol is in.".into(), group: Some(g.clone()) }));
        s.at(2500, Ev::Ims(Timed::Reply { from: "+15550103".into(), text: "Yes! I'll bring snacks.".into(), group: Some(g.clone()) }));
        s.at(4000, Ev::Ims(Timed::ReplyFile { from: "+15550103".into(), group: Some(g) }));
    }
}
