//! SIP (RFC 3261) helpers on top of [`TextMsg`] plus HTTP Digest (RFC 2617).
use crate::md5::md5_hex;
use crate::msg::TextMsg;
use crate::prelude::*;

pub const ICSI_CPM_MSG: &str = "urn%3Aurn-7%3A3gpp-service.ims.icsi.oma.cpm.msg";
pub const IARI_FT_HTTP: &str = "urn%3Aurn-7%3A3gpp-application.ims.iari.rcs.fthttp";

/// Build a response to `req`, copying the dialog-identifying headers.
pub fn response_to(req: &TextMsg, code: u16, reason: &str, to_tag: Option<&str>) -> TextMsg {
    let mut r = TextMsg::new(&format!("SIP/2.0 {} {}", code, reason));
    for v in req.headers_all("Via") {
        r.add("Via", v);
    }
    if let Some(f) = req.header("From") {
        r.add("From", f);
    }
    if let Some(t) = req.header("To") {
        match to_tag {
            Some(tag) if !t.contains(";tag=") => r.add("To", &format!("{};tag={}", t, tag)),
            _ => r.add("To", t),
        }
    }
    for h in ["Call-ID", "CSeq"] {
        if let Some(v) = req.header(h) {
            r.add(h, v);
        }
    }
    r
}

/// `;name=value` header parameter.
pub fn param(h: &str, name: &str) -> Option<String> {
    for seg in h.split(';').skip(1) {
        let (k, v) = seg.split_once('=').unwrap_or((seg, ""));
        if k.trim().eq_ignore_ascii_case(name) {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// User part of a SIP/tel URI inside a header: `"A" <sip:+1555@host>;tag=x` -> `+1555`.
pub fn uri_user(h: &str) -> String {
    let inner = match (h.find('<'), h.find('>')) {
        (Some(a), Some(b)) if b > a => &h[a + 1..b],
        _ => h.split(';').next().unwrap_or(h),
    };
    let inner = inner.trim();
    let rest = inner.strip_prefix("sip:").or_else(|| inner.strip_prefix("sips:")).or_else(|| inner.strip_prefix("tel:")).unwrap_or(inner);
    rest.split(['@', ';', '>']).next().unwrap_or("").to_string()
}

pub fn cseq_method(m: &TextMsg) -> String {
    m.header("CSeq").and_then(|c| c.split_whitespace().nth(1)).unwrap_or("").to_string()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub qop: Option<String>,
    pub opaque: Option<String>,
    pub algorithm: String,
}

/// Parse `Digest k="v", k2=v2` auth parameters.
pub fn auth_params(h: &str) -> Vec<(String, String)> {
    let body = h.trim();
    let body = body.strip_prefix("Digest").unwrap_or(body);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut q = false;
    let mut items = Vec::new();
    for c in body.chars() {
        match c {
            '"' => {
                q = !q;
                cur.push(c)
            }
            ',' if !q => items.push(core::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    items.push(cur);
    for it in items {
        if let Some((k, v)) = it.split_once('=') {
            out.push((k.trim().to_ascii_lowercase(), v.trim().trim_matches('"').to_string()));
        }
    }
    out
}

pub fn parse_challenge(h: &str) -> Option<Challenge> {
    let p = auth_params(h);
    let get = |k: &str| p.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    Some(Challenge {
        realm: get("realm")?,
        nonce: get("nonce")?,
        qop: get("qop").map(|q| q.split(',').next().unwrap_or("auth").trim().to_string()),
        opaque: get("opaque"),
        algorithm: get("algorithm").unwrap_or_else(|| "MD5".into()),
    })
}

pub fn digest(user: &str, pass: &str, realm: &str, method: &str, uri: &str, nonce: &str, nc: &str, cnonce: &str, qop: Option<&str>) -> String {
    let ha1 = md5_hex(format!("{}:{}:{}", user, realm, pass).as_bytes());
    let ha2 = md5_hex(format!("{}:{}", method, uri).as_bytes());
    match qop {
        Some(q) => md5_hex(format!("{}:{}:{}:{}:{}:{}", ha1, nonce, nc, cnonce, q, ha2).as_bytes()),
        None => md5_hex(format!("{}:{}:{}", ha1, nonce, ha2).as_bytes()),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn authorization(ch: &Challenge, user: &str, pass: &str, method: &str, uri: &str, nc: u32, cnonce: &str) -> String {
    let ncs = format!("{:08x}", nc);
    let resp = digest(user, pass, &ch.realm, method, uri, &ch.nonce, &ncs, cnonce, ch.qop.as_deref());
    let mut s = format!(
        "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", response=\"{}\", algorithm={}",
        user, ch.realm, ch.nonce, uri, resp, ch.algorithm
    );
    if let Some(q) = &ch.qop {
        s.push_str(&format!(", qop={}, nc={}, cnonce=\"{}\"", q, ncs, cnonce));
    }
    if let Some(o) = &ch.opaque {
        s.push_str(&format!(", opaque=\"{}\"", o));
    }
    s
}

/// Server side: verify an Authorization header against a known password.
pub fn verify(auth: &str, pass: &str, method: &str) -> bool {
    let p = auth_params(auth);
    let get = |k: &str| p.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let qop = get("qop");
    let expect = digest(
        &get("username"),
        pass,
        &get("realm"),
        method,
        &get("uri"),
        &get("nonce"),
        &get("nc"),
        &get("cnonce"),
        if qop.is_empty() { None } else { Some(&qop) },
    );
    expect == get("response")
}

#[cfg(test)]
mod t {
    use super::*;
    #[test]
    fn rfc2617_example() {
        // RFC 2617 §3.5 example
        let r = digest("Mufasa", "Circle Of Life", "testrealm@host.com", "GET", "/dir/index.html", "dcd98b7102dd2f0e8b11d0f600bfb0c093", "00000001", "0a4f113b", Some("auth"));
        assert_eq!(r, "6629fae49393a05397450978507c4ef1");
    }
    #[test]
    fn helpers() {
        assert_eq!(uri_user("\"Alice\" <sip:+15550101@ims.tranquil.net>;tag=abc"), "+15550101");
        assert_eq!(uri_user("<tel:+15550102>"), "+15550102");
        assert_eq!(param("<sip:a@b>;tag=xyz;foo", "tag").as_deref(), Some("xyz"));
        let c = parse_challenge("Digest realm=\"ims\", nonce=\"abc,def\", qop=\"auth,auth-int\", algorithm=MD5").unwrap();
        assert_eq!(c.nonce, "abc,def");
        assert_eq!(c.qop.as_deref(), Some("auth"));
        let a = authorization(&c, "u", "p", "REGISTER", "sip:ims", 1, "cn");
        assert!(verify(&a, "p", "REGISTER"));
        assert!(!verify(&a, "wrong", "REGISTER"));
    }
}
