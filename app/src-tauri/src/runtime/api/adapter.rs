//! The adapter boundary, and what both wire formats share: SSE framing and HTTP status
//! mapping.
use super::types::*;
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::header::HeaderMap;
use std::time::Duration;

#[async_trait]
pub trait ModelAdapter: Send + Sync {
    async fn stream(&self, req: &ModelRequest) -> Result<ModelStream, ApiError>;
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ApiError>;
}

/// Turns one wire format's SSE events into ModelEvents.
pub trait WireParser: Send + 'static {
    fn on(&mut self, ev: &SseEvent) -> Result<Vec<ModelEvent>, ApiError>;
    /// The stream ended. Events not sent yet (a format that ends without a stop event).
    fn finish(&mut self) -> Result<Vec<ModelEvent>, ApiError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Splits a byte stream into SSE events. Chunks may cut lines and UTF-8 anywhere.
#[derive(Default)]
pub struct SseFramer {
    buf: Vec<u8>,
}

impl SseFramer {
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend(chunk.iter().filter(|b| **b != b'\r'));
        let mut out = vec![];
        while let Some(i) = self.buf.windows(2).position(|w| w == b"\n\n") {
            let block: Vec<u8> = self.buf.drain(..i + 2).collect();
            if let Some(e) = parse_block(&String::from_utf8_lossy(&block)) {
                out.push(e);
            }
        }
        out
    }

    /// What is left when the stream ends without a blank line.
    pub fn rest(&mut self) -> Option<SseEvent> {
        let b = std::mem::take(&mut self.buf);
        parse_block(&String::from_utf8_lossy(&b))
    }
}

fn parse_block(block: &str) -> Option<SseEvent> {
    let mut event = None;
    let mut data: Vec<&str> = vec![];
    for line in block.lines() {
        if let Some(v) = line.strip_prefix("event:") {
            event = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("data:") {
            data.push(v.strip_prefix(' ').unwrap_or(v));
        }
    }
    (!data.is_empty()).then(|| SseEvent { event, data: data.join("\n") })
}

/// Runs a response body through the framer and a parser. Stops after the first error.
pub fn drive(resp: reqwest::Response, mut parser: impl WireParser) -> ModelStream {
    let body = Box::pin(resp.bytes_stream());
    let s = async_stream_like(body, move |item, framer: &mut SseFramer| -> Vec<Result<ModelEvent, ApiError>> {
        match item {
            Some(Ok(bytes)) => collect(framer.feed(&bytes).iter().map(|e| parser.on(e))),
            Some(Err(e)) => vec![Err(ApiError::Retryable(format!("The connection broke: {e}")))],
            None => {
                let mut out = framer.rest().map(|e| collect(std::iter::once(parser.on(&e)))).unwrap_or_default();
                match parser.finish() {
                    Ok(v) => out.extend(v.into_iter().map(Ok)),
                    Err(e) => out.push(Err(e)),
                }
                out
            }
        }
    });
    Box::pin(s)
}

fn collect(results: impl Iterator<Item = Result<Vec<ModelEvent>, ApiError>>) -> Vec<Result<ModelEvent, ApiError>> {
    let mut out = vec![];
    for r in results {
        match r {
            Ok(v) => out.extend(v.into_iter().map(Ok)),
            Err(e) => {
                out.push(Err(e));
                break;
            }
        }
    }
    out
}

/// A small unfold: feeds body chunks (then one `None` at the end) to `step`, flattens the
/// results, and ends after an error.
fn async_stream_like<S, F>(body: S, step: F) -> impl futures::Stream<Item = Result<ModelEvent, ApiError>> + Send
where
    S: futures::Stream<Item = reqwest::Result<bytes::Bytes>> + Send + Unpin + 'static,
    F: FnMut(Option<reqwest::Result<bytes::Bytes>>, &mut SseFramer) -> Vec<Result<ModelEvent, ApiError>> + Send + 'static,
{
    struct St<S, F> {
        body: S,
        step: F,
        framer: SseFramer,
        done: bool,
    }
    futures::stream::unfold(St { body, step, framer: SseFramer::default(), done: false }, |mut st| async move {
        if st.done {
            return None;
        }
        let item = st.body.next().await;
        if item.is_none() {
            st.done = true;
        }
        let mut out = (st.step)(item, &mut st.framer);
        if let Some(i) = out.iter().position(|r| r.is_err()) {
            out.truncate(i + 1);
            st.done = true;
        }
        Some((futures::stream::iter(out), st))
    })
    .flatten()
}

/// Redirects are not followed: a key in a custom header (Anthropic's `x-api-key`) would go along.
pub fn http() -> reqwest::Client {
    reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(15)).read_timeout(Duration::from_secs(120)).build().unwrap_or_default()
}

/// Sends a request and maps a non-2xx status to an ApiError.
pub async fn send(req: reqwest::RequestBuilder) -> Result<reqwest::Response, ApiError> {
    let resp = req.send().await.map_err(|e| {
        if e.is_timeout() {
            ApiError::Retryable("The provider did not answer in time.".into())
        } else {
            ApiError::Retryable("The provider could not be reached.".into())
        }
    })?;
    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(resp);
    }
    let until = rate_limit_until(resp.headers(), chrono::Utc::now().timestamp());
    let body = resp.text().await.unwrap_or_default();
    Err(status_error(status, &body, until))
}

pub fn status_error(status: u16, body: &str, until: i64) -> ApiError {
    let detail = error_message(body);
    match status {
        429 => ApiError::RateLimited { until },
        402 => ApiError::Fatal(format!("The provider says the account is out of credits. Add credits, then try again. {}", server_text(&detail))),
        401 | 403 => ApiError::Auth(format!("The provider rejected the key (HTTP {status}) {}", server_text(&detail))),
        408 | 409 | 500..=599 => ApiError::Retryable(format!("The provider had a problem (HTTP {status}) {}", server_text(&detail))),
        _ => ApiError::Fatal(format!("The provider refused the request (HTTP {status}) {}", server_text(&detail))),
    }
}

/// `{"error": {"message": ...}}` (both formats), else the body.
pub fn error_message(body: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let e = &v["error"];
    e["message"].as_str().or(e.as_str()).map(str::to_string).unwrap_or_else(|| body.to_string())
}

/// When to try again: `retry-after` seconds, then the providers' reset headers. Defaults to
/// one minute.
pub fn rate_limit_until(h: &HeaderMap, now: i64) -> i64 {
    let get = |k: &str| h.get(k).and_then(|v| v.to_str().ok());
    if let Some(s) = get("retry-after").and_then(|v| v.trim().parse::<f64>().ok()) {
        return now + s.ceil() as i64;
    }
    for k in ["anthropic-ratelimit-requests-reset", "anthropic-ratelimit-tokens-reset", "anthropic-ratelimit-input-tokens-reset"] {
        if let Some(t) = get(k).and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok()) {
            return t.timestamp().max(now);
        }
    }
    for k in ["x-ratelimit-reset-requests", "x-ratelimit-reset-tokens"] {
        if let Some(d) = get(k).and_then(go_duration) {
            return now + d;
        }
    }
    now + 60
}

/// "1m30s", "6m0s", "20ms", "2.5s" to whole seconds (rounded up).
fn go_duration(s: &str) -> Option<i64> {
    let mut total = 0.0;
    let mut num = String::new();
    let mut chars = s.trim().chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
            continue;
        }
        let n: f64 = num.parse().ok()?;
        num.clear();
        let unit = match c {
            'h' => 3600.0,
            'm' if chars.peek() == Some(&'s') => {
                chars.next();
                0.001
            }
            'm' => 60.0,
            's' => 1.0,
            _ => return None,
        };
        total += n * unit;
        any = true;
    }
    (any && num.is_empty()).then(|| total.ceil() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    #[test]
    fn framer_handles_cut_chunks_and_crlf() {
        let mut f = SseFramer::default();
        assert!(f.feed(b"event: ping\r\ndata: {\"a\"").is_empty());
        let ev = f.feed(b":1}\r\n\r\ndata: x\n\ndata: [DONE]");
        assert_eq!(ev, [SseEvent { event: Some("ping".into()), data: "{\"a\":1}".into() }, SseEvent { event: None, data: "x".into() }]);
        assert_eq!(f.rest(), Some(SseEvent { event: None, data: "[DONE]".into() }));
    }

    #[test]
    fn framer_keeps_utf8_cut_between_chunks() {
        let mut f = SseFramer::default();
        let all = "data: héllo\n\n".as_bytes();
        assert!(f.feed(&all[..8]).is_empty());
        assert_eq!(f.feed(&all[8..])[0].data, "héllo");
    }

    #[test]
    fn statuses_map_to_kinds() {
        assert_eq!(status_error(429, "", 5), ApiError::RateLimited { until: 5 });
        assert!(matches!(status_error(401, r#"{"error":{"message":"bad key"}}"#, 0), ApiError::Auth(m) if m.contains("bad key") && m.contains("not instructions")));
        assert!(matches!(status_error(529, "", 0), ApiError::Retryable(_)));
        assert!(matches!(status_error(402, "need credits", 0), ApiError::Fatal(m) if m.contains("out of credits")));
        assert!(matches!(status_error(400, r#"{"error":"nope"}"#, 0), ApiError::Fatal(m) if m.contains("nope")));
    }

    #[test]
    fn rate_limit_reset_from_headers() {
        let mut h = HeaderMap::new();
        assert_eq!(rate_limit_until(&h, 100), 160);
        h.insert("x-ratelimit-reset-requests", HeaderValue::from_static("1m30s"));
        assert_eq!(rate_limit_until(&h, 100), 190);
        h.insert("anthropic-ratelimit-requests-reset", HeaderValue::from_static("1970-01-01T00:05:00Z"));
        assert_eq!(rate_limit_until(&h, 100), 300);
        h.insert("retry-after", HeaderValue::from_static("7"));
        assert_eq!(rate_limit_until(&h, 100), 107);
        assert_eq!(go_duration("20ms"), Some(1));
        assert_eq!(go_duration("6m0s"), Some(360));
        assert_eq!(go_duration("abc"), None);
    }
}
