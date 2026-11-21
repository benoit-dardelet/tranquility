//! OMA MMS encapsulation (OMA-WAP-MMS-ENC 1.3): binary PDUs exchanged with
//! the MMSC over HTTP, plus the multipart body and the WAP Push wrapper used
//! for m-notification-ind over SMS.
use crate::prelude::*;
use crate::wsp::{self, Reader, Writer, R};

pub const M_SEND_REQ: u8 = 0x80;
pub const M_SEND_CONF: u8 = 0x81;
pub const M_NOTIFICATION_IND: u8 = 0x82;
pub const M_NOTIFYRESP_IND: u8 = 0x83;
pub const M_RETRIEVE_CONF: u8 = 0x84;
pub const M_ACKNOWLEDGE_IND: u8 = 0x85;
pub const M_DELIVERY_IND: u8 = 0x86;

pub const STATUS_RETRIEVED: u8 = 0x81;
pub const RESPONSE_OK: u8 = 0x80;
pub const VERSION_1_3: u8 = 0x13;

/// WAP Push application port for MMS user agent.
pub const WAP_PUSH_PORT: u16 = 2948;
pub const WAP_PUSH_SRC_PORT: u16 = 9200;

// header field codes (without the high bit)
const BCC: u8 = 0x01;
const CC: u8 = 0x02;
const CONTENT_LOCATION: u8 = 0x03;
const CONTENT_TYPE: u8 = 0x04;
const DATE: u8 = 0x05;
const DELIVERY_REPORT: u8 = 0x06;
const EXPIRY: u8 = 0x08;
const FROM: u8 = 0x09;
const MESSAGE_CLASS: u8 = 0x0A;
const MESSAGE_ID: u8 = 0x0B;
const MESSAGE_TYPE: u8 = 0x0C;
const MMS_VERSION: u8 = 0x0D;
const MESSAGE_SIZE: u8 = 0x0E;
const RESPONSE_STATUS: u8 = 0x12;
const RESPONSE_TEXT: u8 = 0x13;
const STATUS: u8 = 0x15;
const SUBJECT: u8 = 0x16;
const TO: u8 = 0x17;
const TRANSACTION_ID: u8 = 0x18;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Part {
    pub content_type: String,
    pub name: String,
    pub data: Vec<u8>,
}

impl Part {
    pub fn text(s: &str) -> Part {
        Part { content_type: "text/plain".into(), name: "text.txt".into(), data: s.as_bytes().to_vec() }
    }
    pub fn is_text(&self) -> bool {
        self.content_type.eq_ignore_ascii_case("text/plain")
    }
    pub fn as_text(&self) -> String {
        String::from_utf8_lossy(&self.data).into_owned()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Pdu {
    pub msg_type: u8,
    pub transaction_id: String,
    pub version: u8,
    pub from: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: Option<String>,
    pub date: Option<u64>,
    pub message_id: Option<String>,
    pub content_location: Option<String>,
    pub response_status: Option<u8>,
    pub response_text: Option<String>,
    pub status: Option<u8>,
    pub message_size: Option<u64>,
    pub expiry_relative: Option<u64>,
    pub delivery_report: Option<bool>,
    pub message_class: Option<u8>,
    pub parts: Vec<Part>,
}

/// "+15550101" -> "+15550101/TYPE=PLMN"
pub fn plmn(addr: &str) -> String {
    if addr.contains('/') || addr.contains('@') {
        addr.to_string()
    } else {
        format!("{}/TYPE=PLMN", addr)
    }
}

pub fn strip_type(addr: &str) -> String {
    addr.split('/').next().unwrap_or(addr).to_string()
}

impl Pdu {
    pub fn new(msg_type: u8, tid: &str) -> Pdu {
        Pdu { msg_type, transaction_id: tid.to_string(), version: VERSION_1_3, ..Default::default() }
    }

    pub fn has_body(&self) -> bool {
        matches!(self.msg_type, M_SEND_REQ | M_RETRIEVE_CONF)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.short_int(MESSAGE_TYPE).short_int(self.msg_type & 0x7F);
        if !self.transaction_id.is_empty() {
            w.short_int(TRANSACTION_ID).text(&self.transaction_id);
        }
        w.short_int(MMS_VERSION).short_int(self.version);
        if let Some(id) = &self.message_id {
            w.short_int(MESSAGE_ID).text(id);
        }
        if let Some(d) = self.date {
            w.short_int(DATE).long_int(d);
        }
        match (&self.from, self.msg_type) {
            (Some(f), _) => {
                let f = plmn(f);
                w.short_int(FROM).with_length(|w| {
                    w.byte(0x80).text(&f);
                });
            }
            (None, M_SEND_REQ) => {
                w.short_int(FROM).byte(1).byte(0x81);
            }
            _ => {}
        }
        for t in &self.to {
            w.short_int(TO).text(&plmn(t));
        }
        for t in &self.cc {
            w.short_int(CC).text(&plmn(t));
        }
        if let Some(s) = &self.subject {
            w.short_int(SUBJECT).text(s);
        }
        if let Some(c) = self.message_class {
            w.short_int(MESSAGE_CLASS).byte(c);
        }
        if let Some(sz) = self.message_size {
            w.short_int(MESSAGE_SIZE).long_int(sz);
        }
        if let Some(e) = self.expiry_relative {
            w.short_int(EXPIRY).with_length(|w| {
                w.byte(0x81).long_int(e);
            });
        }
        if let Some(dr) = self.delivery_report {
            w.short_int(DELIVERY_REPORT).byte(if dr { 0x80 } else { 0x81 });
        }
        if let Some(s) = self.response_status {
            w.short_int(RESPONSE_STATUS).byte(s);
        }
        if let Some(t) = &self.response_text {
            w.short_int(RESPONSE_TEXT).text(t);
        }
        if let Some(s) = self.status {
            w.short_int(STATUS).byte(s);
        }
        if let Some(l) = &self.content_location {
            w.short_int(CONTENT_LOCATION).text(l);
        }
        if self.has_body() {
            // Content-Type must be the last header.
            w.short_int(CONTENT_TYPE).short_int(0x23); // application/vnd.wap.multipart.mixed
            encode_multipart(&mut w, &self.parts);
        }
        w.buf
    }

    pub fn decode(d: &[u8]) -> R<Pdu> {
        let mut r = Reader::new(d);
        let mut p = Pdu { version: VERSION_1_3, ..Default::default() };
        while !r.eof() {
            let field = r.byte()?;
            if field & 0x80 == 0 {
                // application header: token text + value text
                r.p -= 1;
                r.text()?;
                r.text()?;
                continue;
            }
            match field & 0x7F {
                MESSAGE_TYPE => p.msg_type = r.byte()?,
                TRANSACTION_ID => p.transaction_id = r.text()?,
                MMS_VERSION => p.version = r.byte()? & 0x7F,
                MESSAGE_ID => p.message_id = Some(r.text()?),
                DATE => p.date = Some(r.long_int()?),
                FROM => {
                    let len = r.value_length()?;
                    let end = r.p + len;
                    let tok = r.byte()?;
                    if tok == 0x80 {
                        p.from = Some(strip_type(&r.encoded_string()?));
                    }
                    r.p = end;
                }
                TO => p.to.push(strip_type(&r.encoded_string()?)),
                CC => p.cc.push(strip_type(&r.encoded_string()?)),
                BCC => {
                    r.encoded_string()?;
                }
                SUBJECT => p.subject = Some(r.encoded_string()?),
                MESSAGE_CLASS => {
                    if r.peek()? >= 0x80 {
                        p.message_class = Some(r.byte()?);
                    } else {
                        r.text()?;
                        p.message_class = Some(0x80);
                    }
                }
                MESSAGE_SIZE => p.message_size = Some(r.long_int()?),
                EXPIRY => {
                    let len = r.value_length()?;
                    let end = r.p + len;
                    let tok = r.byte()?;
                    let v = r.long_int()?;
                    if tok == 0x81 {
                        p.expiry_relative = Some(v);
                    }
                    r.p = end;
                }
                DELIVERY_REPORT => p.delivery_report = Some(r.byte()? == 0x80),
                RESPONSE_STATUS => p.response_status = Some(r.byte()?),
                RESPONSE_TEXT => p.response_text = Some(r.encoded_string()?),
                STATUS => p.status = Some(r.byte()?),
                CONTENT_LOCATION => p.content_location = Some(r.text()?),
                CONTENT_TYPE => {
                    let (_ct, _params) = decode_content_type(&mut r)?;
                    p.parts = decode_multipart(r.rest())?;
                    break;
                }
                _ => {
                    r.skip_value()?;
                }
            }
        }
        Ok(p)
    }

    pub fn text(&self) -> String {
        let mut s = String::new();
        for part in self.parts.iter().filter(|p| p.is_text()) {
            if !s.is_empty() {
                s.push('\n');
            }
            s.push_str(&part.as_text());
        }
        s
    }
}

fn encode_content_type(w: &mut Writer, ct: &str, name: &str) {
    let is_text = ct.starts_with("text/");
    if !is_text && name.is_empty() {
        match wsp::content_type_code(ct) {
            Some(c) => w.short_int(c),
            None => w.text(ct),
        };
        return;
    }
    w.with_length(|w| {
        match wsp::content_type_code(ct) {
            Some(c) => w.short_int(c),
            None => w.text(ct),
        };
        if is_text {
            w.short_int(0x01).short_int(106); // Charset = utf-8
        }
        if !name.is_empty() {
            w.short_int(0x05).text(name); // Name (v1.1 encoding)
        }
    });
}

/// Returns (media type, name parameter).
fn decode_content_type(r: &mut Reader) -> R<(String, String)> {
    let b = r.peek()?;
    if b >= 0x80 {
        r.p += 1;
        return Ok((wsp::content_type_name(b & 0x7F), String::new()));
    }
    if b >= 0x20 {
        return Ok((r.text()?, String::new()));
    }
    let len = r.value_length()?;
    let end = r.p + len;
    let media = if r.peek()? >= 0x80 { wsp::content_type_name(r.byte()? & 0x7F) } else { r.text()? };
    let mut name = String::new();
    while r.p < end {
        let param = r.byte()?;
        match param {
            0x81 => {
                r.integer()?;
            }
            0x85 | 0x97 => name = r.text()?,
            0x89 | 0x8A => {
                r.text()?;
            }
            _ if param < 0x80 => {
                r.p -= 1;
                r.text()?;
                r.skip_value()?;
            }
            _ => {
                r.skip_value()?;
            }
        }
    }
    r.p = end;
    Ok((media, name))
}

fn encode_multipart(w: &mut Writer, parts: &[Part]) {
    w.uintvar(parts.len() as u32);
    for p in parts {
        let mut h = Writer::new();
        encode_content_type(&mut h, &p.content_type, if p.is_text() { "" } else { &p.name });
        if !p.name.is_empty() {
            h.short_int(0x0E).text(&p.name); // Content-Location
        }
        w.uintvar(h.buf.len() as u32).uintvar(p.data.len() as u32).bytes(&h.buf).bytes(&p.data);
    }
}

fn decode_multipart(d: &[u8]) -> R<Vec<Part>> {
    let mut r = Reader::new(d);
    if r.eof() {
        return Ok(Vec::new());
    }
    let n = r.uintvar()?;
    let mut parts = Vec::new();
    for _ in 0..n {
        let hlen = r.uintvar()? as usize;
        let dlen = r.uintvar()? as usize;
        let hdr = r.take(hlen)?;
        let data = r.take(dlen)?.to_vec();
        let mut hr = Reader::new(hdr);
        let (content_type, mut name) = decode_content_type(&mut hr)?;
        while !hr.eof() {
            let f = hr.byte()?;
            if f == 0x8E {
                name = hr.text()?;
            } else if f & 0x80 == 0 {
                hr.p -= 1;
                hr.text()?;
                hr.text()?;
            } else {
                hr.skip_value()?;
            }
        }
        parts.push(Part { content_type, name, data });
    }
    Ok(parts)
}

/// Wrap an MMS PDU in a WSP Push PDU (connectionless, TID 1).
pub fn wap_push(mms: &[u8]) -> Vec<u8> {
    let headers = [0xBE, 0xAF, 0x84]; // Content-Type mms-message, X-Wap-Application-Id: mms.ua
    let mut w = Writer::new();
    w.byte(0x01).byte(0x06).uintvar(headers.len() as u32).bytes(&headers).bytes(mms);
    w.buf
}

/// Unwrap a WSP (confirmed) push. Returns the body if it is an MMS message.
pub fn parse_wap_push(d: &[u8]) -> Option<Vec<u8>> {
    let mut r = Reader::new(d);
    let _tid = r.byte().ok()?;
    let ty = r.byte().ok()?;
    if ty != 0x06 && ty != 0x07 {
        return None;
    }
    let hlen = r.uintvar().ok()? as usize;
    let hdr = r.take(hlen).ok()?;
    let mut hr = Reader::new(hdr);
    let (ct, _) = decode_content_type(&mut hr).ok()?;
    if !ct.eq_ignore_ascii_case("application/vnd.wap.mms-message") {
        return None;
    }
    Some(r.rest().to_vec())
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn send_req_roundtrip() {
        let mut p = Pdu::new(M_SEND_REQ, "T123");
        p.to.push("+15550101".into());
        p.subject = Some("Hi".into());
        p.date = Some(1_700_000_000);
        p.delivery_report = Some(false);
        p.message_class = Some(0x80);
        p.parts.push(Part::text("hello mms"));
        p.parts.push(Part { content_type: "image/bmp".into(), name: "smile.bmp".into(), data: vec![1, 2, 3, 0, 255] });
        let enc = p.encode();
        assert_eq!(&enc[..2], &[0x8C, 0x80]);
        let d = Pdu::decode(&enc).unwrap();
        assert_eq!(d.msg_type, M_SEND_REQ);
        assert_eq!(d.transaction_id, "T123");
        assert_eq!(d.to, vec!["+15550101".to_string()]);
        assert_eq!(d.from, None);
        assert_eq!(d.parts, p.parts);
        assert_eq!(d.text(), "hello mms");
    }
    #[test]
    fn notification_push_roundtrip() {
        let mut n = Pdu::new(M_NOTIFICATION_IND, "N1");
        n.from = Some("+15550102".into());
        n.message_class = Some(0x80);
        n.message_size = Some(1234);
        n.expiry_relative = Some(604800);
        n.content_location = Some("http://mmsc.tranquil.net/m/TQ1".into());
        let push = wap_push(&n.encode());
        let body = parse_wap_push(&push).unwrap();
        assert_eq!(Pdu::decode(&body).unwrap(), n);
    }
}
