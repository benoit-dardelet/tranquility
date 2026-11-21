//! Minimal MIME multipart (RFC 2046) and multipart/form-data (RFC 7578),
//! used by RCS group messages (RFC 5365 recipient lists) and RCS file
//! transfer uploads.
use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Part {
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Part {
    pub fn new(content_type: &str, body: &[u8]) -> Part {
        Part { headers: vec![("Content-Type".into(), content_type.into())], body: body.to_vec() }
    }
    pub fn h(mut self, n: &str, v: &str) -> Part {
        self.headers.push((n.into(), v.into()));
        self
    }
    pub fn header(&self, n: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.as_str())
    }
    pub fn content_type(&self) -> &str {
        self.header("Content-Type").unwrap_or("text/plain")
    }
}

/// Encode parts; returns (Content-Type header value, body).
pub fn encode(subtype: &str, boundary: &str, parts: &[Part]) -> (String, Vec<u8>) {
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        for (n, v) in &p.headers {
            out.extend_from_slice(format!("{}: {}\r\n", n, v).as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&p.body);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());
    (format!("multipart/{};boundary={}", subtype, boundary), out)
}

pub fn boundary_of(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim().eq_ignore_ascii_case("boundary").then(|| v.trim().trim_matches('"').to_string())
    })
}

fn find(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (from..=h.len() - n.len()).find(|&i| &h[i..i + n.len()] == n)
}

pub fn decode(content_type: &str, body: &[u8]) -> Vec<Part> {
    let b = match boundary_of(content_type) {
        Some(b) => b,
        None => return Vec::new(),
    };
    let delim = format!("--{}", b).into_bytes();
    let mut parts = Vec::new();
    let mut pos = match find(body, &delim, 0) {
        Some(p) => p + delim.len(),
        None => return parts,
    };
    loop {
        if body.get(pos..pos + 2) == Some(b"--") {
            break;
        }
        // skip CRLF after delimiter
        while pos < body.len() && (body[pos] == b'\r' || body[pos] == b'\n') {
            pos += 1;
            if body.get(pos - 1) == Some(&b'\n') {
                break;
            }
        }
        let next = match find(body, &delim, pos) {
            Some(n) => n,
            None => break,
        };
        let mut chunk = &body[pos..next];
        if chunk.ends_with(b"\r\n") {
            chunk = &chunk[..chunk.len() - 2];
        }
        let (head, data) = match find(chunk, b"\r\n\r\n", 0) {
            Some(i) => (&chunk[..i], &chunk[i + 4..]),
            None => (&chunk[..0], chunk),
        };
        let mut p = Part { headers: Vec::new(), body: data.to_vec() };
        for l in String::from_utf8_lossy(head).split("\r\n") {
            if let Some((n, v)) = l.split_once(':') {
                p.headers.push((n.trim().to_string(), v.trim().to_string()));
            }
        }
        parts.push(p);
        pos = next + delim.len();
    }
    parts
}

/// `name="x"` parameter of a header such as Content-Disposition.
pub fn param(h: &str, name: &str) -> Option<String> {
    h.split(';').skip(1).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim().trim_matches('"').to_string())
    })
}

// ------------------------------------------------------------- resource lists (RFC 4826 / 5365)

pub const CT_RESOURCE_LISTS: &str = "application/resource-lists+xml";

pub fn resource_list(uris: &[String]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<resource-lists xmlns=\"urn:ietf:params:xml:ns:resource-lists\" xmlns:cp=\"urn:ietf:params:xml:ns:copycontrol\"><list>");
    for u in uris {
        s.push_str(&format!("<entry uri=\"{}\" cp:copyControl=\"to\"/>", u));
    }
    s.push_str("</list></resource-lists>");
    s
}

pub fn parse_resource_list(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find("uri=\"") {
        rest = &rest[i + 5..];
        if let Some(j) = rest.find('"') {
            out.push(rest[..j].to_string());
            rest = &rest[j..];
        }
    }
    out
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn roundtrip() {
        let parts = vec![
            Part::new(CT_RESOURCE_LISTS, resource_list(&["sip:a@x".into(), "sip:b@x".into()]).as_bytes()).h("Content-Disposition", "recipient-list"),
            Part::new("message/cpim", b"hello\r\n\r\nbody\r\n--not-a-boundary"),
        ];
        let (ct, body) = encode("mixed", "tqb0und", &parts);
        let d = decode(&ct, &body);
        assert_eq!(d, parts);
        assert_eq!(parse_resource_list(&String::from_utf8_lossy(&d[0].body)), vec!["sip:a@x", "sip:b@x"]);
        assert_eq!(param("form-data; name=\"File\"; filename=\"a.bmp\"", "filename").as_deref(), Some("a.bmp"));
    }
}
