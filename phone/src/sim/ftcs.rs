//! Simulated RCS File Transfer content server (HTTP): accepts
//! multipart/form-data uploads and serves downloads.
use super::{Ev, Sched};
use crate::bmp;
use crate::mime;
use crate::msg::TextMsg;
use crate::prelude::*;
use crate::services::ft::{file_info_xml, FileInfo, CT_FT};
use crate::util::iso8601;

pub const HOST: &str = "ftcs.tranquil.net";

#[derive(Default)]
pub struct Ftcs {
    rx: BTreeMap<u8, Vec<u8>>,
    files: BTreeMap<String, (String, Vec<u8>)>,
    counter: u32,
}

impl Ftcs {
    pub fn on_close(&mut self, conn: u8) {
        self.rx.remove(&conn);
    }

    /// Store a file directly (used by bots) and return its file-info XML.
    pub fn store(&mut self, name: &str, ctype: &str, data: Vec<u8>, now_unix: i64) -> String {
        self.counter += 1;
        let id = format!("f{:05}", self.counter);
        let size = data.len() as u64;
        self.files.insert(id.clone(), (ctype.to_string(), data));
        file_info_xml(&FileInfo { name: name.into(), content_type: ctype.into(), size, url: format!("http://{}/dl/{}", HOST, id), until: iso8601(now_unix + 7 * 86400) })
    }

    pub fn bot_picture(&mut self, now_unix: i64) -> String {
        self.store("heart.bmp", "image/bmp", bmp::encode(&bmp::HEART), now_unix)
    }

    pub fn on_data(&mut self, conn: u8, data: &[u8], s: &mut Sched) {
        let buf = self.rx.entry(conn).or_default();
        buf.extend_from_slice(data);
        let req = match TextMsg::parse(buf) {
            Ok(Some((r, n))) => {
                buf.drain(..n);
                r
            }
            _ => return,
        };
        let close = |m: TextMsg| m.h("Connection", "close");
        let resp = match (req.method(), req.uri()) {
            (Some("POST"), Some("/upload")) => {
                let ct = req.header("Content-Type").unwrap_or("").to_string();
                let file = mime::decode(&ct, &req.body).into_iter().find(|p| p.header("Content-Disposition").is_some_and(|d| mime::param(d, "name").as_deref() == Some("File")));
                match file {
                    Some(f) => {
                        let name = f.header("Content-Disposition").and_then(|d| mime::param(d, "filename")).unwrap_or_else(|| "file".into());
                        let ctype = f.content_type().to_string();
                        let xml = self.store(&name, &ctype, f.body, s.unix());
                        close(TextMsg::new("HTTP/1.1 200 OK").body(CT_FT, xml.as_bytes()))
                    }
                    None => close(TextMsg::new("HTTP/1.1 400 Bad Request")),
                }
            }
            (Some("GET"), Some(uri)) if uri.starts_with("/dl/") => match self.files.get(&uri[4..]) {
                Some((ct, d)) => close(TextMsg::new("HTTP/1.1 200 OK").body(ct, d)),
                None => close(TextMsg::new("HTTP/1.1 404 Not Found")),
            },
            _ => close(TextMsg::new("HTTP/1.1 404 Not Found")),
        };
        s.at(250, Ev::SockData(conn, resp.to_bytes()));
    }
}
