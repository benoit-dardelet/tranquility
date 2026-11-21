//! RCS File Transfer over HTTP (GSMA RCC.07 §3.2.5): the file is uploaded
//! to the FT content server, and the returned file-info XML is sent to the
//! peer (or group) as a chat message. The receiver downloads it from the
//! URL in the XML.
use super::http::Purpose;
use super::*;
use crate::mime;
use crate::msg::TextMsg;

pub const CT_FT: &str = "application/vnd.gsma.rcs-ft-http+xml";

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FileInfo {
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub url: String,
    pub until: String,
}

pub fn file_info_xml(f: &FileInfo) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<file xmlns=\"urn:gsma:params:xml:ns:rcs:rcs:fthttp\"><file-info type=\"file\"><file-size>{}</file-size><file-name>{}</file-name><content-type>{}</content-type><data url=\"{}\" until=\"{}\"/></file-info></file>",
        f.size, f.name, f.content_type, f.url, f.until
    )
}

pub fn parse_file_info(xml: &str) -> Option<FileInfo> {
    // Use the "file" file-info, not a thumbnail.
    let start = xml.find("type=\"file\"").unwrap_or(0);
    let x = &xml[start..];
    let url = {
        let i = x.find("url=\"")? + 5;
        let j = x[i..].find('"')?;
        x[i..i + j].to_string()
    };
    Some(FileInfo {
        name: util::xml_text(x, "file-name").unwrap_or("file").trim().to_string(),
        content_type: util::xml_text(x, "content-type").unwrap_or("application/octet-stream").trim().to_string(),
        size: util::xml_text(x, "file-size").and_then(|s| s.trim().parse().ok()).unwrap_or(0),
        url,
        until: String::new(),
    })
}

fn split_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, "/".to_string()),
    };
    let (host, port) = match hostport.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None => (hostport.to_string(), 80),
    };
    Some((host, port, path))
}

impl Core {
    /// Upload attachment `part` of message `msg` to the FT content server.
    pub(crate) fn ft_upload(&mut self, msg: u32, part: usize) {
        let p = match self.find_msg(msg).and_then(|m| m.attachments.get(part).cloned()) {
            Some(p) => p,
            None => return,
        };
        let tid = self.rng.token(12);
        let boundary = format!("tqft{}", self.rng.token(12));
        let (ct, body) = mime::encode(
            "form-data",
            &boundary,
            &[
                mime::Part { headers: vec![("Content-Disposition".into(), "form-data; name=\"tid\"".into())], body: tid.into_bytes() },
                mime::Part::new(&p.content_type, &p.data).h("Content-Disposition", &format!("form-data; name=\"File\"; filename=\"{}\"", p.name)),
            ],
        );
        let req = TextMsg::request("POST", "/upload", "HTTP/1.1").body(&ct, &body);
        self.http(CID_INTERNET, FTCS_HOST, 80, req, Purpose::FtUpload { msg, part });
    }

    pub(crate) fn ft_uploaded(&mut self, msg: u32, part: usize, resp: Option<TextMsg>) {
        let xml = match resp.map(|r| r.body_str()).filter(|x| parse_file_info(x).is_some()) {
            Some(x) => x,
            None => {
                // Upload failed: deliver the whole message by MMS instead.
                if part == 0 {
                    self.rcs_fallback(msg);
                }
                return;
            }
        };
        let (key, group) = match self.threads.iter().find(|t| t.msgs.iter().any(|m| m.id == msg)) {
            Some(t) => (t.number.clone(), t.is_group()),
            None => return,
        };
        if group {
            self.rcs_group_payload(msg, &key, CT_FT, &xml);
        } else {
            self.rcs_payload(msg, &key, CT_FT, &xml);
        }
        let more = self.find_msg(msg).map(|m| m.attachments.len()).unwrap_or(0);
        if part + 1 < more {
            self.ft_upload(msg, part + 1);
        }
    }

    /// Incoming file-info: create the message and fetch the file.
    pub(crate) fn ft_incoming(&mut self, key: &str, from: &str, imdn: &str, wants_display: bool, xml: &str) {
        let f = match parse_file_info(xml) {
            Some(f) => f,
            None => return,
        };
        let mut m = self.new_msg(true, Transport::Rcs, "");
        m.imdn_id = imdn.to_string();
        m.wants_display = wants_display;
        m.from = from.to_string();
        m.subject = Some(format!("File: {} ({} B)", f.name, f.size));
        let id = m.id;
        self.push_incoming(key, m);
        if let Some((host, port, path)) = split_url(&f.url) {
            let req = TextMsg::request("GET", &path, "HTTP/1.1");
            self.http(CID_INTERNET, &host, port, req, Purpose::FtDownload { msg: id, name: f.name, content_type: f.content_type });
        }
    }

    pub(crate) fn ft_downloaded(&mut self, msg: u32, name: String, content_type: String, resp: Option<TextMsg>) {
        match (resp, self.find_msg(msg)) {
            (Some(r), Some(m)) => {
                m.subject = None;
                m.attachments.push(Part { content_type, name, data: r.body });
            }
            (None, Some(m)) => m.subject = Some(format!("File: {} (download failed)", name)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn file_info() {
        let f = FileInfo { name: "a.bmp".into(), content_type: "image/bmp".into(), size: 42, url: "http://ftcs.tranquil.net/dl/x1".into(), until: "2026-11-28T00:00:00Z".into() };
        assert_eq!(parse_file_info(&file_info_xml(&f)).map(|mut g| { g.until = f.until.clone(); g }), Some(f));
        assert_eq!(split_url("http://h.net:8080/a/b"), Some(("h.net".into(), 8080, "/a/b".into())));
    }
}
