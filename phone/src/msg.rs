//! Generic RFC 822-style message framing shared by HTTP/1.1 and SIP over TCP:
//! start line, headers, Content-Length delimited body.
use crate::prelude::*;
use crate::util::eq_ic;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextMsg {
    pub start: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// SIP compact header forms (RFC 3261 §7.3.3).
fn canon(name: &str) -> &str {
    match name {
        "f" | "F" => "From",
        "t" | "T" => "To",
        "i" | "I" => "Call-ID",
        "v" | "V" => "Via",
        "m" | "M" => "Contact",
        "l" | "L" => "Content-Length",
        "c" | "C" => "Content-Type",
        "k" | "K" => "Supported",
        "a" | "A" => "Accept-Contact",
        _ => name,
    }
}

impl TextMsg {
    pub fn new(start: &str) -> TextMsg {
        TextMsg { start: start.to_string(), headers: Vec::new(), body: Vec::new() }
    }
    pub fn request(method: &str, uri: &str, proto: &str) -> TextMsg {
        TextMsg::new(&format!("{} {} {}", method, uri, proto))
    }
    pub fn h(mut self, name: &str, value: &str) -> TextMsg {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }
    pub fn add(&mut self, name: &str, value: &str) {
        self.headers.push((name.to_string(), value.to_string()));
    }
    pub fn set(&mut self, name: &str, value: &str) {
        self.headers.retain(|(n, _)| !eq_ic(canon(n), name));
        self.add(name, value);
    }
    pub fn body(mut self, content_type: &str, body: &[u8]) -> TextMsg {
        self.set("Content-Type", content_type);
        self.body = body.to_vec();
        self
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| eq_ic(canon(n), name)).map(|(_, v)| v.as_str())
    }
    pub fn headers_all(&self, name: &str) -> Vec<&str> {
        self.headers.iter().filter(|(n, _)| eq_ic(canon(n), name)).map(|(_, v)| v.as_str()).collect()
    }
    pub fn is_response(&self) -> bool {
        self.start.starts_with("SIP/2.0 ") || self.start.starts_with("HTTP/1.")
    }
    pub fn status(&self) -> Option<u16> {
        if !self.is_response() {
            return None;
        }
        self.start.split(' ').nth(1)?.parse().ok()
    }
    pub fn method(&self) -> Option<&str> {
        if self.is_response() {
            None
        } else {
            self.start.split(' ').next()
        }
    }
    pub fn uri(&self) -> Option<&str> {
        if self.is_response() {
            None
        } else {
            self.start.split(' ').nth(1)
        }
    }
    pub fn body_str(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut s = String::new();
        s.push_str(&self.start);
        s.push_str("\r\n");
        for (n, v) in &self.headers {
            if eq_ic(canon(n), "Content-Length") {
                continue;
            }
            s.push_str(n);
            s.push_str(": ");
            s.push_str(v);
            s.push_str("\r\n");
        }
        s.push_str(&format!("Content-Length: {}\r\n\r\n", self.body.len()));
        let mut out = s.into_bytes();
        out.extend_from_slice(&self.body);
        out
    }

    /// Try to parse one complete message from the front of `buf`.
    /// `Ok(None)` means more bytes are needed; on success returns bytes consumed.
    pub fn parse(buf: &[u8]) -> Result<Option<(TextMsg, usize)>, ()> {
        let mut skip = 0;
        while skip < buf.len() && (buf[skip] == b'\r' || buf[skip] == b'\n') {
            skip += 1; // SIP keep-alive CRLFs
        }
        let b = &buf[skip..];
        let end = match b.windows(4).position(|w| w == b"\r\n\r\n") {
            Some(e) => e,
            None => return if b.len() > 64 * 1024 { Err(()) } else { Ok(None) },
        };
        let head = String::from_utf8_lossy(&b[..end]);
        let mut lines = head.split("\r\n");
        let start = lines.next().ok_or(())?.to_string();
        let mut m = TextMsg::new(&start);
        for l in lines {
            if let Some((n, v)) = l.split_once(':') {
                m.headers.push((n.trim().to_string(), v.trim().to_string()));
            }
        }
        let cl: usize = m.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(0);
        let body_start = end + 4;
        if b.len() < body_start + cl {
            return Ok(None);
        }
        m.body = b[body_start..body_start + cl].to_vec();
        Ok(Some((m, skip + body_start + cl)))
    }
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn roundtrip_partial() {
        let m = TextMsg::request("POST", "/mms", "HTTP/1.1").h("Host", "x").body("text/plain", b"hello");
        let b = m.to_bytes();
        assert_eq!(TextMsg::parse(&b[..b.len() - 1]), Ok(None));
        let (p, n) = TextMsg::parse(&b).unwrap().unwrap();
        assert_eq!(n, b.len());
        assert_eq!(p.body, b"hello");
        assert_eq!(p.header("content-type"), Some("text/plain"));
        let mut two = b"\r\n\r\n".to_vec();
        two.extend(&b);
        assert_eq!(TextMsg::parse(&two).unwrap().unwrap().1, two.len());
    }
}
