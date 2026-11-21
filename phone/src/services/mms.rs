//! MMS user agent (MM1): sends m-send-req via HTTP POST to the MMSC,
//! handles WAP-Push m-notification-ind, retrieves with HTTP GET and
//! acknowledges with m-notifyresp-ind. Group MMS uses several To headers.
use super::http::Purpose;
use super::*;
use crate::mms::{self as enc, Pdu};
use crate::msg::TextMsg;

#[derive(Default)]
pub struct MmsAgent {
    pub sent: u32,
    pub received: u32,
}

const CT_MMS: &str = "application/vnd.wap.mms-message";

impl Core {
    fn mms_post(&mut self, pdu: Vec<u8>, p: Purpose) {
        let req = TextMsg::request("POST", MMSC_PATH, "HTTP/1.1")
            .h("Accept", "application/vnd.wap.mms-message, */*")
            .h("x-wap-profile", "\"http://tranquil.net/uaprof/tq5g1.xml\"")
            .body(CT_MMS, &pdu);
        self.http(CID_MMS, MMSC_HOST, 80, req, p);
    }

    pub(crate) fn mms_send(&mut self, id: u32, to: &[String], text: &str, atts: Vec<Part>) {
        let tid = format!("T{}", self.rng.token(10));
        let mut p = Pdu::new(enc::M_SEND_REQ, &tid);
        p.to = to.to_vec();
        p.date = Some(self.unix() as u64);
        p.message_class = Some(0x80);
        p.delivery_report = Some(self.settings.delivery_reports);
        if let Some(t) = self.threads.iter().find(|t| t.msgs.iter().any(|m| m.id == id)) {
            if t.is_group() {
                p.subject = Some(t.group_name.clone());
            }
        }
        if !text.is_empty() {
            p.parts.push(Part::text(text));
        }
        p.parts.extend(atts);
        if let Some(m) = self.find_msg(id) {
            m.transport = Transport::Mms;
        }
        self.mms_post(p.encode(), Purpose::MmsSend { msg: id });
    }

    pub(crate) fn mms_sent(&mut self, msg: u32, resp: Option<TextMsg>) {
        let good = resp.and_then(|r| Pdu::decode(&r.body).ok()).is_some_and(|c| c.response_status == Some(enc::RESPONSE_OK));
        if let Some(m) = self.find_msg(msg) {
            m.status = if good { Status::Sent } else { Status::Failed };
        }
        if good {
            self.mms.sent += 1;
        }
    }

    pub(crate) fn mms_on_push(&mut self, body: &[u8]) {
        let p = match Pdu::decode(body) {
            Ok(p) if p.msg_type == enc::M_NOTIFICATION_IND => p,
            _ => return,
        };
        let loc = match p.content_location {
            Some(l) => l,
            None => return,
        };
        let path = match loc.strip_prefix("http://") {
            Some(rest) => rest.find('/').map(|i| rest[i..].to_string()).unwrap_or_else(|| "/".into()),
            None => loc.clone(),
        };
        let req = TextMsg::request("GET", &path, "HTTP/1.1").h("Accept", "application/vnd.wap.mms-message, */*");
        self.http(CID_MMS, MMSC_HOST, 80, req, Purpose::MmsRetrieve { tid: p.transaction_id });
    }

    pub(crate) fn mms_retrieved(&mut self, tid: &str, resp: Option<TextMsg>) {
        let p = match resp.and_then(|r| Pdu::decode(&r.body).ok()) {
            Some(p) => p,
            None => return self.toast("MMS download failed"),
        };
        let from = p.from.clone().unwrap_or_else(|| "Unknown".into());
        let mut m = self.new_msg(true, Transport::Mms, &p.text());
        m.subject = p.subject.clone();
        m.from = from.clone();
        m.attachments = p.parts.iter().filter(|x| !x.is_text() && !x.content_type.contains("smil")).cloned().collect();
        if let Some(d) = p.date {
            m.time = d as i64;
        }
        // Group MMS: everyone on To/Cc plus the sender, minus me.
        let me = self.info.number.clone();
        let mut people: Vec<String> = p.to.iter().chain(p.cc.iter()).cloned().collect();
        people.push(from.clone());
        people.retain(|x| !same_number(x, &me));
        people.sort();
        people.dedup();
        let key = if people.len() > 1 { self.create_group(p.subject.as_deref().unwrap_or(""), &people, None) } else { from };
        self.mms.received += 1;
        self.push_incoming(&key, m);
        let mut ack = Pdu::new(enc::M_NOTIFYRESP_IND, tid);
        ack.status = Some(enc::STATUS_RETRIEVED);
        self.mms_post(ack.encode(), Purpose::MmsAck);
    }

    pub fn mms_pending(&self) -> usize {
        self.http_pending()
    }
}
