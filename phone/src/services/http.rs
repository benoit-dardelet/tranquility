//! Small HTTP/1.1 client over the modem's TCP sockets. One connection per
//! transaction (`Connection: close`); used by the MMS agent and RCS file
//! transfer.
use super::*;
use crate::msg::TextMsg;

#[derive(Clone, Debug)]
pub enum Purpose {
    MmsSend { msg: u32 },
    MmsRetrieve { tid: String },
    MmsAck,
    FtUpload { msg: u32, part: usize },
    FtDownload { msg: u32, name: String, content_type: String },
}

#[derive(Debug)]
struct Tx {
    id: u32,
    purpose: Purpose,
    conn: Option<u8>,
    req: Vec<u8>,
    rx: Vec<u8>,
    started: u64,
}

#[derive(Default)]
pub struct HttpAgent {
    txs: Vec<Tx>,
    next: u32,
}

impl Core {
    pub(crate) fn http(&mut self, cid: u8, host: &str, port: u16, req: TextMsg, purpose: Purpose) {
        self.http.next += 1;
        let id = self.http.next;
        let req = req.h("Host", host).h("User-Agent", "Tranquility/0.2").h("Connection", "close").to_bytes();
        let conn = self.sock_open(cid, host, port, Owner::Http(id));
        let failed = conn.is_none();
        self.http.txs.push(Tx { id, purpose, conn, req, rx: Vec::new(), started: self.now });
        if failed {
            self.http_finish(id, None);
        }
    }

    pub fn http_pending(&self) -> usize {
        self.http.txs.len()
    }

    pub(crate) fn http_connected(&mut self, tx: u32, conn: u8) {
        if let Some(req) = self.http.txs.iter().find(|t| t.id == tx).map(|t| t.req.clone()) {
            self.sock_send(conn, &req);
        }
    }

    pub(crate) fn http_data(&mut self, tx: u32, conn: u8, d: &[u8]) {
        let resp = match self.http.txs.iter_mut().find(|t| t.id == tx) {
            Some(t) => {
                t.rx.extend_from_slice(d);
                match TextMsg::parse(&t.rx) {
                    Ok(Some((m, _))) => Some(m),
                    Ok(None) => return,
                    Err(()) => None,
                }
            }
            None => return,
        };
        self.sock_close(conn);
        self.http_finish(tx, resp);
    }

    pub(crate) fn http_closed(&mut self, tx: u32) {
        self.http_finish(tx, None);
    }

    fn http_finish(&mut self, tx: u32, resp: Option<TextMsg>) {
        let pos = match self.http.txs.iter().position(|t| t.id == tx) {
            Some(p) => p,
            None => return,
        };
        let t = self.http.txs.remove(pos);
        if let (Some(c), None) = (t.conn, &resp) {
            self.sock_close(c);
        }
        let resp = resp.filter(|r| r.status().is_some_and(|s| (200..300).contains(&s)));
        match t.purpose {
            Purpose::MmsSend { msg } => self.mms_sent(msg, resp),
            Purpose::MmsRetrieve { tid } => self.mms_retrieved(&tid, resp),
            Purpose::MmsAck => {}
            Purpose::FtUpload { msg, part } => self.ft_uploaded(msg, part, resp),
            Purpose::FtDownload { msg, name, content_type } => self.ft_downloaded(msg, name, content_type, resp),
        }
    }

    pub(crate) fn http_tick(&mut self) {
        let now = self.now;
        let stale: Vec<u32> = self.http.txs.iter().filter(|t| now.saturating_sub(t.started) > 30_000).map(|t| t.id).collect();
        for id in stale {
            self.http_finish(id, None);
        }
    }

    pub(crate) fn http_reset(&mut self) {
        let ids: Vec<u32> = self.http.txs.iter().map(|t| t.id).collect();
        for id in ids {
            self.http_finish(id, None);
        }
    }
}
