//! Plain-text email in and out of Gmail's base64url format. Pure.
use base64::engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use serde_json::Value;

/// A plain-text email bleep writes. Addresses are already checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Outgoing {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

/// RFC 5322 message, base64url, ready for Gmail's `raw`. Refuses line breaks in headers.
pub fn build_raw(m: &Outgoing) -> Result<String, String> {
    let clean = |v: &str| -> Result<String, String> {
        if v.contains(['\r', '\n']) { Err("Line breaks are not allowed in email headers.".into()) } else { Ok(v.to_string()) }
    };
    let subject = clean(&m.subject)?;
    let subject = if subject.is_ascii() { subject } else { format!("=?UTF-8?B?{}?=", STANDARD.encode(subject.as_bytes())) };
    let mut out = String::from("MIME-Version: 1.0\r\n");
    out += &format!("To: {}\r\n", m.to.iter().map(|a| clean(a)).collect::<Result<Vec<_>, _>>()?.join(", "));
    if !m.cc.is_empty() {
        out += &format!("Cc: {}\r\n", m.cc.iter().map(|a| clean(a)).collect::<Result<Vec<_>, _>>()?.join(", "));
    }
    out += &format!("Subject: {subject}\r\n");
    if let Some(v) = &m.in_reply_to {
        out += &format!("In-Reply-To: {}\r\n", clean(v)?);
    }
    if let Some(v) = &m.references {
        out += &format!("References: {}\r\n", clean(v)?);
    }
    out += "Content-Type: text/plain; charset=\"UTF-8\"\r\nContent-Transfer-Encoding: base64\r\n\r\n";
    let body = m.body.replace("\r\n", "\n").replace('\n', "\r\n");
    let encoded = STANDARD.encode(body.as_bytes());
    for chunk in encoded.as_bytes().chunks(76) {
        out += std::str::from_utf8(chunk).expect("base64 is ascii");
        out += "\r\n";
    }
    Ok(URL_SAFE.encode(out.as_bytes()))
}

/// The readable text of a message payload: text/plain if there is one, else text/html without tags.
pub fn body_text(payload: &Value) -> String {
    if let Some(t) = find_part(payload, "text/plain") {
        return t.trim().to_string();
    }
    find_part(payload, "text/html").map(|h| strip_html(&h)).unwrap_or_default()
}

fn find_part(p: &Value, mime: &str) -> Option<String> {
    if p["mimeType"] == mime {
        if let Some(d) = p["body"]["data"].as_str() {
            return decode(d);
        }
    }
    p["parts"].as_array()?.iter().find_map(|c| find_part(c, mime))
}

fn decode(data: &str) -> Option<String> {
    let bytes = URL_SAFE_NO_PAD.decode(data.trim_end_matches('=')).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Enough HTML to text for reading a mail: drops style and script, keeps line breaks.
fn strip_html(html: &str) -> String {
    let mut s = html.to_string();
    for tag in ["style", "script", "head"] {
        while let Some(a) = s.to_ascii_lowercase().find(&format!("<{tag}")) {
            let close = format!("</{tag}>");
            let b = s.to_ascii_lowercase()[a..].find(&close).map(|i| a + i + close.len()).unwrap_or(s.len());
            s.replace_range(a..b, "");
        }
    }
    let mut out = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match (in_tag, c) {
            (false, '<') => {
                in_tag = true;
                tag.clear();
            }
            (true, '>') => {
                in_tag = false;
                let t = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("").to_ascii_lowercase();
                if t == "br" || (tag.starts_with('/') && matches!(t.as_str(), "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3")) {
                    out.push('\n');
                }
            }
            (true, c) => tag.push(c),
            (false, c) => out.push(c),
        }
    }
    let out = out.replace("&nbsp;", " ").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&");
    let lines: Vec<&str> = out.lines().map(str::trim).collect();
    lines.join("\n").split("\n\n\n").collect::<Vec<_>>().join("\n\n").trim().to_string()
}

/// A header value from a message payload, by name, any case.
pub fn header<'a>(payload: &'a Value, name: &str) -> Option<&'a str> {
    payload["headers"].as_array()?.iter().find(|h| h["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name)))?["value"].as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use serde_json::json;

    fn decode(raw: &str) -> String {
        String::from_utf8(base64::engine::general_purpose::URL_SAFE.decode(raw).unwrap()).unwrap()
    }

    fn mail() -> Outgoing {
        Outgoing { to: vec!["priya@example.com".into()], cc: vec![], subject: "Deck review".into(), body: "Sure, tomorrow.".into(), in_reply_to: None, references: None }
    }

    #[test]
    fn raw_has_headers_and_utf8_body() {
        let mut m = mail();
        m.cc = vec!["arjun@example.com".into()];
        m.subject = "Rent ₹2,340".into();
        m.body = "Paid ₹2,340.\nThanks".into();
        m.in_reply_to = Some("<deck-2@example.com>".into());
        m.references = Some("<deck-1@example.com> <deck-2@example.com>".into());
        let text = decode(&build_raw(&m).unwrap());
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        assert!(head.contains("To: priya@example.com\r\n"), "{head}");
        assert!(head.contains("Cc: arjun@example.com\r\n"));
        assert!(head.contains("Subject: =?UTF-8?B?"), "non-ASCII subject is encoded: {head}");
        assert!(head.contains("In-Reply-To: <deck-2@example.com>\r\n"));
        assert!(head.contains("References: <deck-1@example.com> <deck-2@example.com>\r\n"));
        assert!(head.contains("Content-Type: text/plain; charset=\"UTF-8\""));
        assert!(head.contains("Content-Transfer-Encoding: base64"));
        let plain = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(body.replace("\r\n", "")).unwrap()).unwrap();
        assert_eq!(plain, "Paid ₹2,340.\r\nThanks");
        assert!(!decode(&build_raw(&mail()).unwrap()).contains("Cc:"));
    }

    #[test]
    fn header_injection_is_refused() {
        let mut m = mail();
        m.subject = "Hi\r\nBcc: spy@example.org".into();
        assert!(build_raw(&m).is_err());
        let mut m = mail();
        m.to = vec!["a@b.co\nBcc: spy@example.org".into()];
        assert!(build_raw(&m).is_err());
        let mut m = mail();
        m.in_reply_to = Some("<x>\r\nBcc: y".into());
        assert!(build_raw(&m).is_err());
    }

    #[test]
    fn body_text_prefers_plain_then_strips_html() {
        let b = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
        let alt = json!({"mimeType": "multipart/alternative", "parts": [
            {"mimeType": "text/html", "body": {"data": b("<p>html</p>")}},
            {"mimeType": "text/plain", "body": {"data": b("plain text")}}
        ]});
        assert_eq!(body_text(&alt), "plain text");
        let html = json!({"mimeType": "text/html", "body": {"data": b("<div>Sure, <i>tomorrow</i>.<br>Thanks &amp; regards</div><style>p{}</style>")}});
        assert_eq!(body_text(&html), "Sure, tomorrow.\nThanks & regards");
        let nested = json!({"mimeType": "multipart/mixed", "parts": [{"mimeType": "multipart/alternative", "parts": [{"mimeType": "text/plain", "body": {"data": b("deep")}}]}, {"mimeType": "application/pdf", "filename": "bill.pdf", "body": {"attachmentId": "a1"}}]});
        assert_eq!(body_text(&nested), "deep");
        assert_eq!(body_text(&json!({})), "");
    }
}
