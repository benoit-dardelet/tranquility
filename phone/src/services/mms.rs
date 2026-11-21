//! MMS user agent (MM1): sends m-send-req via HTTP POST to the MMSC,
//! handles WAP-Push m-notification-ind, retrieves with HTTP GET and
//! acknowledges with m-notifyresp-ind.
use super::*;
use crate::mms::{self as enc, Pdu};
use crate::msg::TextMsg;

#[derive(Debug)]
enum Op {
    Send { msg: u32, pdu: Vec<u8> },
    Retrieve { path: String, tid: String },
    NotifyResp { pdu: Vec<u8> },
}

#[derive(Debug)]
struct Tx {
    id: u32,
    op: Op,
    conn: Option<u8>,
    rx: Vec<u8>,
    started: u64,
}

#[derive(Default)]
pub struct MmsAgent {
    txs: Vec<Tx>,
    next: u32,
    pub sent: u32,
    pub received: u32,
}

impl Core {
    pub(crate) fn mms_send(&mut self, id: u32, to: &str, text: &str, atts: Vec<Part>) {
        let tid = format!("T{}", self.rng.token(10));
        let mut p = Pdu::new(enc::M_SEND_REQ, &tid);
        p.to.push(to.to_string());
        p.date = Some(self.unix() as u64);
        p.message_class = Some(0x80);
        p.delivery_report = Some(self.settings.delivery_reports);
        if !text.is_empty() {
            p.parts.push(Part::text(text));
        }
        p.parts.extend(atts);
        self.mms_queue(Op::Send { msg: id, pdu: p.encode() });
    }

    fn mms_queue(&mut self, op: Op) {
        self.mms.next += 1;
        let id = self.mms.next;
        let conn = self.sock_open(CID_MMS, MMSC_HOST, 80, Owner::Mms(id));
        let failed = conn.is_none();
        self.mms.txs.push(Tx { id, op, conn, rx: Vec::new(), started: self.now });
        if failed {
            self.mms_fail(id);
        }
    }

    pub(crate) fn mms_connected(&mut self, tx: u32, conn: u8) {
        let req = match self.mms.txs.iter().find(|t| t.id == tx) {
            Some(t) => match &t.op {
                Op::Send { pdu, .. } | Op::NotifyResp { pdu } => TextMsg::request("POST", MMSC_PATH, "HTTP/1.1").body("application/vnd.wap.mms-message", pdu),
                Op::Retrieve { path, .. } => TextMsg::request("GET", path, "HTTP/1.1"),
            },
            None => return,
        };
        let req = req
            .h("Host", MMSC_HOST)
            .h("User-Agent", "Tranquility/0.1 MMS/1.3")
            .h("Accept", "application/vnd.wap.mms-message, */*")
            .h("x-wap-profile", "\"http://tranquil.net/uaprof/tq5g1.xml\"")
            .h("Connection", "close");
        let b = req.to_bytes();
        self.sock_send(conn, &b);
    }

    pub(crate) fn mms_data(&mut self, tx: u32, conn: u8, d: &[u8]) {
        let resp = match self.mms.txs.iter_mut().find(|t| t.id == tx) {
            Some(t) => {
                t.rx.extend_from_slice(d);
                match TextMsg::parse(&t.rx) {
                    Ok(Some((m, _))) => m,
                    Ok(None) => return,
                    Err(()) => TextMsg::new("HTTP/1.1 999 Bad"),
                }
            }
            None => return,
        };
        self.sock_close(conn);
        let pos = match self.mms.txs.iter().position(|t| t.id == tx) {
            Some(p) => p,
            None => return,
        };
        let t = self.mms.txs.remove(pos);
        let ok = resp.status().is_some_and(|s| (200..300).contains(&s));
        match t.op {
            Op::Send { msg, .. } => {
                let conf = Pdu::decode(&resp.body).ok();
                let good = ok && conf.as_ref().is_some_and(|c| c.response_status == Some(enc::RESPONSE_OK));
                if let Some(m) = self.find_msg(msg) {
                    m.status = if good { Status::Sent } else { Status::Failed };
                }
                if good {
                    self.mms.sent += 1;
                }
            }
            Op::Retrieve { tid, .. } => {
                if !ok {
                    self.toast("MMS download failed");
                    return;
                }
                if let Ok(p) = Pdu::decode(&resp.body) {
                    let from = p.from.clone().unwrap_or_else(|| "Unknown".into());
                    let mut m = self.new_msg(true, Transport::Mms, &p.text());
                    m.subject = p.subject.clone();
                    m.attachments = p.parts.iter().filter(|x| !x.is_text() && !x.content_type.contains("smil")).cloned().collect();
                    if let Some(d) = p.date {
                        m.time = d as i64;
                    }
                    self.mms.received += 1;
                    self.push_incoming(&from, m);
                    let mut ack = Pdu::new(enc::M_NOTIFYRESP_IND, &tid);
                    ack.status = Some(enc::STATUS_RETRIEVED);
                    self.mms_queue(Op::NotifyResp { pdu: ack.encode() });
                }
            }
            Op::NotifyResp { .. } => {}
        }
    }

    pub(crate) fn mms_closed(&mut self, tx: u32) {
        if self.mms.txs.iter().any(|t| t.id == tx) {
            self.mms_fail(tx);
        }
    }

    fn mms_fail(&mut self, tx: u32) {
        let pos = match self.mms.txs.iter().position(|t| t.id == tx) {
            Some(p) => p,
            None => return,
        };
        let t = self.mms.txs.remove(pos);
        if let Some(c) = t.conn {
            self.sock_close(c);
        }
        match t.op {
            Op::Send { msg, .. } => {
                if let Some(m) = self.find_msg(msg) {
                    m.status = Status::Failed;
                }
            }
            Op::Retrieve { .. } => self.toast("MMS download failed"),
            Op::NotifyResp { .. } => {}
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
        self.mms_queue(Op::Retrieve { path, tid: p.transaction_id });
    }

    pub(crate) fn mms_tick(&mut self) {
        let now = self.now;
        let stale: Vec<u32> = self.mms.txs.iter().filter(|t| now.saturating_sub(t.started) > 30_000).map(|t| t.id).collect();
        for id in stale {
            self.mms_fail(id);
        }
    }

    pub(crate) fn mms_reset(&mut self) {
        let ids: Vec<u32> = self.mms.txs.iter().map(|t| t.id).collect();
        for id in ids {
            self.mms_fail(id);
        }
    }

    pub fn mms_pending(&self) -> usize {
        self.mms.txs.len()
    }
}
