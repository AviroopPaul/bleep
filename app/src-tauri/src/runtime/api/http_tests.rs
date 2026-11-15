//! Both adapters over real HTTP, against a local server that replays recorded streams.
use super::adapter::ModelAdapter;
use super::anthropic::AnthropicAdapter;
use super::openai::OpenAiAdapter;
use super::types::*;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use futures::StreamExt;
use serde_json::json;
use std::sync::{Arc, Mutex};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/fixtures/api/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

async fn server() -> (String, Seen) {
    let seen: Seen = Default::default();
    let s1 = seen.clone();
    let s2 = seen.clone();
    let app = axum::Router::new()
        .route(
            "/v1/messages",
            post(move |h: HeaderMap, body: String| async move {
                s1.lock().unwrap().push((body, h.get("x-api-key").and_then(|v| v.to_str().ok()).map(str::to_string)));
                ([("content-type", "text/event-stream")], fixture("anthropic_text.sse"))
            }),
        )
        .route(
            "/oa/chat/completions",
            post(move |h: HeaderMap| async move {
                s2.lock().unwrap().push((String::new(), h.get("authorization").and_then(|v| v.to_str().ok()).map(str::to_string)));
                ([("content-type", "text/event-stream")], fixture("openai_tool.sse"))
            }),
        )
        .route("/limited/chat/completions", post(|| async { (StatusCode::TOO_MANY_REQUESTS, [("retry-after", "30")], r#"{"error":{"message":"slow down"}}"#) }))
        .route("/oa/models", get(|| async { axum::Json(json!({"data": [{"id": "gpt-5-mini"}]})) }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, seen)
}

fn req() -> ModelRequest {
    ModelRequest { model: "m".into(), system: "s".into(), msgs: vec![Msg::user_text("hi")], tools: vec![], choice: ToolChoice::Auto, max_tokens: 100 }
}

#[tokio::test]
async fn anthropic_streams_over_http_with_its_key_header() {
    let (url, seen) = server().await;
    let ev: Vec<_> = AnthropicAdapter::new(&url, "sk-ant").stream(&req()).await.unwrap().collect().await;
    assert_eq!(ev.last(), Some(&Ok(ModelEvent::Stop(StopReason::EndTurn))));
    let s = seen.lock().unwrap();
    assert_eq!(s[0].1.as_deref(), Some("sk-ant"));
    assert!(s[0].0.contains(r#""stream":true"#));
}

#[tokio::test]
async fn openai_streams_over_http_with_a_bearer_key() {
    let (url, seen) = server().await;
    let a = OpenAiAdapter::new(&format!("{url}/oa"), "sk-or", false);
    let ev: Vec<_> = a.stream(&req()).await.unwrap().collect().await;
    assert!(ev.iter().any(|e| matches!(e, Ok(ModelEvent::ToolCall { name, .. }) if name == "bleep_now")));
    assert_eq!(seen.lock().unwrap()[0].1.as_deref(), Some("Bearer sk-or"));
    assert_eq!(a.list_models().await.unwrap()[0].id, "gpt-5-mini");
}

#[tokio::test]
async fn a_429_is_rate_limited_until_retry_after() {
    let (url, _) = server().await;
    let before = chrono::Utc::now().timestamp();
    let Err(e) = OpenAiAdapter::new(&format!("{url}/limited"), "", false).stream(&req()).await else { panic!("expected an error") };
    assert!(matches!(e, ApiError::RateLimited { until } if until >= before + 30 && until <= before + 32), "{e:?}");
}

#[tokio::test]
async fn an_unreachable_server_is_retryable() {
    let Err(e) = OpenAiAdapter::new("http://127.0.0.1:1", "", false).stream(&req()).await else { panic!() };
    assert!(matches!(e, ApiError::Retryable(_)));
}
