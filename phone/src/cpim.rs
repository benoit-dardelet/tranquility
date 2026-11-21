//! RCS / OMA CPM message payloads: Message/CPIM (RFC 3862), IMDN (RFC 5438)
//! and isComposing (RFC 3994).
use crate::prelude::*;
use crate::util::xml_text;

pub const CT_CPIM: &str = "message/cpim";
pub const CT_IMDN: &str = "message/imdn+xml";
pub const CT_COMPOSING: &str = "application/im-iscomposing+xml";
pub const CT_TEXT: &str = "text/plain;charset=UTF-8";

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Cpim {
    pub headers: Vec<(String, String)>,
    pub content_type: String,
    pub body: String,
}

impl Cpim {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn message_id(&self) -> Option<&str> {
        self.header("imdn.Message-ID")
    }
    pub fn wants(&self, what: &str) -> bool {
        self.header("imdn.Disposition-Notification").is_some_and(|d| d.contains(what))
    }
    pub fn is_text(&self) -> bool {
        self.content_type.to_ascii_lowercase().starts_with("text/plain")
    }
}

pub struct CpimOut<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub msg_id: &'a str,
    pub datetime: &'a str,
    pub disposition: Option<&'a str>,
    pub content_type: &'a str,
    pub body: &'a str,
}

pub fn encode(m: &CpimOut) -> String {
    let mut s = format!("From: <{}>\r\nTo: <{}>\r\nNS: imdn <urn:ietf:params:imdn>\r\nimdn.Message-ID: {}\r\nDateTime: {}\r\n", m.from, m.to, m.msg_id, m.datetime);
    if let Some(d) = m.disposition {
        s.push_str(&format!("imdn.Disposition-Notification: {}\r\n", d));
    }
    s.push_str(&format!("\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n{}", m.content_type, m.body.len(), m.body));
    s
}

pub fn decode(s: &str) -> Option<Cpim> {
    let (msg_hdrs, rest) = s.split_once("\r\n\r\n")?;
    let (mime_hdrs, body) = rest.split_once("\r\n\r\n").unwrap_or((rest, ""));
    let mut c = Cpim::default();
    for l in msg_hdrs.lines() {
        if let Some((n, v)) = l.split_once(':') {
            c.headers.push((n.trim().to_string(), v.trim().to_string()));
        }
    }
    for l in mime_hdrs.lines() {
        if let Some((n, v)) = l.split_once(':') {
            if n.trim().eq_ignore_ascii_case("Content-Type") {
                c.content_type = v.trim().to_string();
            }
        }
    }
    c.body = body.to_string();
    Some(c)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    Delivered,
    Displayed,
}

pub fn imdn_xml(msg_id: &str, datetime: &str, d: Disposition) -> String {
    let inner = match d {
        Disposition::Delivered => "<delivery-notification><status><delivered/></status></delivery-notification>",
        Disposition::Displayed => "<display-notification><status><displayed/></status></display-notification>",
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<imdn xmlns=\"urn:ietf:params:xml:ns:imdn\"><message-id>{}</message-id><datetime>{}</datetime>{}</imdn>",
        msg_id, datetime, inner
    )
}

pub fn parse_imdn(xml: &str) -> Option<(String, Disposition)> {
    let id = xml_text(xml, "message-id")?.trim().to_string();
    if xml.contains("<displayed") {
        Some((id, Disposition::Displayed))
    } else if xml.contains("<delivered") {
        Some((id, Disposition::Delivered))
    } else {
        None
    }
}

pub fn iscomposing_xml(active: bool) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n<isComposing xmlns=\"urn:ietf:params:xml:ns:im-iscomposing\"><state>{}</state><contenttype>text/plain</contenttype><refresh>60</refresh></isComposing>",
        if active { "active" } else { "idle" }
    )
}

pub fn parse_iscomposing(xml: &str) -> Option<bool> {
    Some(xml_text(xml, "state")?.trim() == "active")
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn cpim_roundtrip() {
        let s = encode(&CpimOut { from: "sip:anonymous@anonymous.invalid", to: "sip:anonymous@anonymous.invalid", msg_id: "abc123", datetime: "2026-11-21T12:00:00Z", disposition: Some("positive-delivery, display"), content_type: CT_TEXT, body: "Hello\r\n\r\nworld" });
        let c = decode(&s).unwrap();
        assert_eq!(c.message_id(), Some("abc123"));
        assert!(c.wants("display"));
        assert!(c.is_text());
        assert_eq!(c.body, "Hello\r\n\r\nworld");
        assert_eq!(parse_imdn(&imdn_xml("abc123", "x", Disposition::Displayed)), Some(("abc123".into(), Disposition::Displayed)));
        assert_eq!(parse_iscomposing(&iscomposing_xml(true)), Some(true));
    }
}
