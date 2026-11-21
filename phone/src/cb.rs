//! 3GPP TS 23.041 cell broadcast (GSM CB page format) — used for public
//! warning systems (WEA / EU-Alert / ETWS).
use crate::gsm7;
use crate::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CbMessage {
    pub serial: u16,
    pub msg_id: u16,
    pub page: u8,
    pub pages: u8,
    pub text: String,
}

/// Decode one 88-octet CB page.
pub fn decode(p: &[u8]) -> Option<CbMessage> {
    if p.len() < 7 {
        return None;
    }
    let serial = (p[0] as u16) << 8 | p[1] as u16;
    let msg_id = (p[2] as u16) << 8 | p[3] as u16;
    let dcs = p[4];
    let page = p[5] >> 4;
    let pages = p[5] & 0xF;
    let content = &p[6..];
    let text = if dcs & 0xF0 == 0x40 && dcs & 0x0C == 0x08 || dcs == 0x48 {
        let units: Vec<u16> = content.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0] as u16) << 8 | c[1] as u16).collect();
        String::from_utf16_lossy(&units)
    } else {
        let n = content.len() * 8 / 7;
        gsm7::decode(&gsm7::unpack(content, 0, n))
    };
    let text = text.trim_end_matches(['\r', '\n', '@']).to_string();
    Some(CbMessage { serial, msg_id, page, pages, text })
}

/// Encode a single-page GSM 7-bit CB message (pads with CR to 93 septets).
pub fn encode(msg_id: u16, serial: u16, text: &str) -> Vec<u8> {
    let mut sept = gsm7::encode(text).unwrap_or_default();
    sept.truncate(93);
    while sept.len() < 93 {
        sept.push(0x0D);
    }
    let mut out = vec![(serial >> 8) as u8, serial as u8, (msg_id >> 8) as u8, msg_id as u8, 0x0F, 0x11];
    let mut body = gsm7::pack(&sept, 0);
    body.resize(82, 0);
    out.extend(body);
    out
}

/// Human-readable category of a CBS message identifier (TS 23.041 §9.4.1.2.2).
pub fn category(msg_id: u16) -> &'static str {
    match msg_id {
        4352 => "Earthquake warning",
        4353 => "Tsunami warning",
        4354 => "Earthquake & tsunami",
        4355 => "ETWS test",
        4356 => "Emergency warning",
        4370 | 4383 => "Presidential alert",
        4371..=4372 | 4384..=4385 => "Extreme alert",
        4373..=4378 | 4386..=4391 => "Severe alert",
        4379 | 4392 => "AMBER alert",
        4380 | 4393 => "Required monthly test",
        4381 | 4394 => "Exercise",
        4396..=4399 => "Public safety",
        _ => "Cell broadcast",
    }
}

pub fn is_emergency(msg_id: u16) -> bool {
    (4352..=4400).contains(&msg_id)
}

#[cfg(test)]
mod t {
    #[test]
    fn roundtrip() {
        let p = super::encode(4370, 0x3001, "Test alert: shelter in place.");
        assert_eq!(p.len(), 88);
        let m = super::decode(&p).unwrap();
        assert_eq!(m.msg_id, 4370);
        assert_eq!(m.text, "Test alert: shelter in place.");
        assert_eq!(super::category(m.msg_id), "Presidential alert");
    }
}
