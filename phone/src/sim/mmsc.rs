//! Simulated MMSC (MM1 interface over HTTP): accepts m-send-req, stores
//! messages, notifies recipients by WAP Push over SMS and serves
//! m-retrieve-conf on GET.
use super::{bots, Ev, Sched, MMSC_HOST, OWN_NUMBER};
use crate::bmp;
use crate::mms::{self, Part, Pdu};
use crate::msg::TextMsg;
use crate::prelude::*;
use crate::util::same_number;

#[derive(Clone, Debug)]
pub enum Timed {
    BotReply { from: String, text: String },
}

#[derive(Default)]
pub struct Mmsc {
    rx: BTreeMap<u8, Vec<u8>>,
    store: BTreeMap<String, Vec<u8>>,
    counter: u32,
}

const CT_MMS: &str = "application/vnd.wap.mms-message";

impl Mmsc {
    pub fn on_close(&mut self, conn: u8) {
        self.rx.remove(&conn);
    }

    pub fn on_data(&mut self, conn: u8, data: &[u8], s: &mut Sched) {
        let buf = self.rx.entry(conn).or_default();
        buf.extend_from_slice(data);
        if let Ok(Some((req, n))) = TextMsg::parse(buf) {
            buf.drain(..n);
            let resp = self.http(&req, s);
            s.at(300, Ev::SockData(conn, resp.to_bytes()));
        }
    }

    fn http(&mut self, req: &TextMsg, s: &mut Sched) -> TextMsg {
        let ok = |body: Vec<u8>| TextMsg::new("HTTP/1.1 200 OK").h("Connection", "close").body(CT_MMS, &body);
        match (req.method(), req.uri()) {
            (Some("GET"), Some(uri)) => {
                let id = uri.rsplit('/').next().unwrap_or("");
                match self.store.get(id) {
                    Some(b) => ok(b.clone()),
                    None => TextMsg::new("HTTP/1.1 404 Not Found").h("Connection", "close"),
                }
            }
            (Some("POST"), _) => {
                let pdu = match Pdu::decode(&req.body) {
                    Ok(p) => p,
                    Err(_) => return TextMsg::new("HTTP/1.1 400 Bad Request").h("Connection", "close"),
                };
                match pdu.msg_type {
                    mms::M_SEND_REQ => {
                        self.counter += 1;
                        let id = format!("TQ{:05}", self.counter);
                        let mut conf = Pdu::new(mms::M_SEND_CONF, &pdu.transaction_id);
                        conf.response_status = Some(mms::RESPONSE_OK);
                        conf.message_id = Some(id);
                        for to in &pdu.to {
                            if same_number(to, OWN_NUMBER) {
                                let mut copy = pdu.clone();
                                copy.from = Some(OWN_NUMBER.into());
                                self.deliver(copy, s);
                            } else if let Some(b) = bots::find(to) {
                                if let Some(text) = bots::reply(b, &pdu.text(), false) {
                                    s.at(4000, Ev::Mms(Timed::BotReply { from: b.number.into(), text }));
                                }
                            }
                        }
                        ok(conf.encode())
                    }
                    _ => TextMsg::new("HTTP/1.1 204 No Content").h("Connection", "close"),
                }
            }
            _ => TextMsg::new("HTTP/1.1 405 Method Not Allowed").h("Connection", "close"),
        }
    }

    /// Store a message for the phone and send the m-notification-ind push.
    fn deliver(&mut self, mut m: Pdu, s: &mut Sched) {
        self.counter += 1;
        let id = format!("TQ{:05}", self.counter);
        m.msg_type = mms::M_RETRIEVE_CONF;
        m.transaction_id = String::new();
        m.message_id = Some(id.clone());
        m.date = Some(s.unix() as u64);
        let body = m.encode();
        let size = body.len() as u64;
        self.store.insert(id.clone(), body);
        let mut n = Pdu::new(mms::M_NOTIFICATION_IND, &format!("N{}", self.counter));
        n.from = m.from.clone();
        n.subject = m.subject.clone();
        n.message_class = Some(0x80);
        n.message_size = Some(size);
        n.expiry_relative = Some(7 * 86400);
        n.content_location = Some(format!("http://{}/m/{}", MMSC_HOST, id));
        s.at(500, Ev::WapPush { from: m.from.unwrap_or_default(), data: mms::wap_push(&n.encode()) });
    }

    pub fn timed(&mut self, t: Timed, s: &mut Sched) {
        match t {
            Timed::BotReply { from, text } => self.bot_mms(&from, &text, s),
        }
    }

    fn bot_mms(&mut self, from: &str, text: &str, s: &mut Sched) {
        let mut m = Pdu::new(mms::M_RETRIEVE_CONF, "");
        m.from = Some(from.to_string());
        m.to.push(OWN_NUMBER.into());
        m.subject = Some("Picture".into());
        m.parts.push(Part::text(text));
        m.parts.push(Part { content_type: "image/bmp".into(), name: "smiley.bmp".into(), data: bmp::encode(&bmp::SMILEY) });
        self.deliver(m, s);
    }

    pub fn inject_from_bot(&mut self, from: &str, s: &mut Sched) {
        self.bot_mms(from, "Check out this picture!", s);
    }
}
