//! Anthropic Messages API with streaming. Caches the system prompt, the tools and the
//! conversation so far (three cache breakpoints).
use super::adapter::*;
use super::types::*;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const VERSION: &str = "2023-06-01";

pub struct AnthropicAdapter {
    pub base_url: String,
    pub key: String,
    pub http: reqwest::Client,
}

impl AnthropicAdapter {
    pub fn new(base_url: &str, key: &str) -> Self {
        Self { base_url: base_url.trim_end_matches('/').to_string(), key: key.to_string(), http: http() }
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, format!("{}/v1/{path}", self.base_url)).header("x-api-key", &self.key).header("anthropic-version", VERSION)
    }
}

#[async_trait]
impl ModelAdapter for AnthropicAdapter {
    async fn stream(&self, req: &ModelRequest) -> Result<ModelStream, ApiError> {
        let resp = send(self.req(reqwest::Method::POST, "messages").json(&body(req))).await?;
        Ok(drive(resp, Parser::default()))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ApiError> {
        let resp = send(self.req(reqwest::Method::GET, "models?limit=1000")).await?;
        let v: Value = resp.json().await.map_err(|e| ApiError::Fatal(format!("The model list could not be read: {e}")))?;
        Ok(parse_models(&v))
    }
}

pub fn parse_models(v: &Value) -> Vec<ModelInfo> {
    v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.to_string();
                    let window = m["max_input_tokens"].as_u64().or_else(|| m["context_window"].as_u64());
                    Some(ModelInfo { price: super::prices::builtin(&id), name: m["display_name"].as_str().map(str::to_string), window, id })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn cache() -> Value {
    json!({"type": "ephemeral"})
}

pub fn body(r: &ModelRequest) -> Value {
    let mut msgs: Vec<Value> = r.msgs.iter().map(message).collect();
    // The third breakpoint: the end of the conversation so far, so the next turn reads it.
    if let Some(last) = msgs.last_mut().and_then(|m| m["content"].as_array_mut()).and_then(|c| c.last_mut()) {
        last["cache_control"] = cache();
    }
    let mut b = json!({
        "model": r.model,
        "max_tokens": r.max_tokens,
        "stream": true,
        "system": [{"type": "text", "text": r.system, "cache_control": cache()}],
        "messages": msgs,
    });
    if !r.tools.is_empty() {
        let mut tools: Vec<Value> = r.tools.iter().map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.schema})).collect();
        if let Some(last) = tools.last_mut() {
            last["cache_control"] = cache();
        }
        b["tools"] = Value::Array(tools);
        b["tool_choice"] = match &r.choice {
            ToolChoice::Auto => json!({"type": "auto"}),
            ToolChoice::None => json!({"type": "none"}),
            ToolChoice::Force(n) => json!({"type": "tool", "name": n}),
        };
    }
    if r.system.is_empty() {
        b.as_object_mut().unwrap().remove("system");
    }
    b
}

fn message(m: &Msg) -> Value {
    let role = match m.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let content: Vec<Value> = m
        .parts
        .iter()
        .map(|p| match p {
            Part::Text { text } => json!({"type": "text", "text": text}),
            Part::Image { media_type, data_b64 } => json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data_b64}}),
            Part::ToolCall { id, name, args } => json!({"type": "tool_use", "id": id, "name": name, "input": args}),
            Part::ToolResult { id, ok, text } => json!({"type": "tool_result", "tool_use_id": id, "content": text, "is_error": !ok}),
        })
        .collect();
    json!({"role": role, "content": content})
}

#[derive(Default)]
pub struct Parser {
    tokens: Tokens,
    stop: Option<StopReason>,
    /// Open tool_use blocks by index: id, name, streamed JSON.
    tools: BTreeMap<u64, (String, String, String)>,
    ended: bool,
}

impl WireParser for Parser {
    fn on(&mut self, ev: &SseEvent) -> Result<Vec<ModelEvent>, ApiError> {
        let v: Value = match serde_json::from_str(&ev.data) {
            Ok(v) => v,
            Err(_) => return Ok(vec![]),
        };
        let kind = v["type"].as_str().or(ev.event.as_deref()).unwrap_or("");
        let mut out = vec![];
        match kind {
            "message_start" => {
                let u = &v["message"]["usage"];
                self.tokens.input = u["input_tokens"].as_u64().unwrap_or(0);
                self.tokens.cache_write = u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                self.tokens.cache_read = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                self.tokens.output = u["output_tokens"].as_u64().unwrap_or(0);
            }
            "content_block_start" => {
                let b = &v["content_block"];
                if b["type"] == "tool_use" {
                    let idx = v["index"].as_u64().unwrap_or(0);
                    self.tools.insert(idx, (b["id"].as_str().unwrap_or("").into(), b["name"].as_str().unwrap_or("").into(), String::new()));
                }
            }
            "content_block_delta" => {
                let d = &v["delta"];
                match d["type"].as_str() {
                    Some("text_delta") => {
                        if let Some(t) = d["text"].as_str().filter(|t| !t.is_empty()) {
                            out.push(ModelEvent::TextDelta(t.into()));
                        }
                    }
                    Some("input_json_delta") => {
                        if let Some(t) = self.tools.get_mut(&v["index"].as_u64().unwrap_or(0)) {
                            t.2.push_str(d["partial_json"].as_str().unwrap_or(""));
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some((id, name, raw)) = self.tools.remove(&v["index"].as_u64().unwrap_or(0)) {
                    out.push(tool_call(id, name, &raw)?);
                }
            }
            "message_delta" => {
                if let Some(o) = v["usage"]["output_tokens"].as_u64() {
                    self.tokens.output = o;
                }
                self.stop = v["delta"]["stop_reason"].as_str().map(stop_reason);
            }
            "message_stop" => out.extend(self.end(StopReason::EndTurn)?),
            "error" => {
                let e = &v["error"];
                let msg = format!("The provider stopped with an error {}", server_text(e["message"].as_str().unwrap_or("")));
                return Err(match e["type"].as_str() {
                    Some("overloaded_error" | "api_error") => ApiError::Retryable(msg),
                    Some("rate_limit_error") => ApiError::RateLimited { until: chrono::Utc::now().timestamp() + 60 },
                    Some("authentication_error" | "permission_error") => ApiError::Auth(msg),
                    _ if is_context_full(&msg) => ApiError::ContextFull(msg),
                    _ => ApiError::Fatal(msg),
                });
            }
            _ => {}
        }
        Ok(out)
    }

    fn finish(&mut self) -> Result<Vec<ModelEvent>, ApiError> {
        self.end_of_body()
    }
}

impl Parser {
    /// The stop event came: what is left to send. `fallback` when no stop reason was given.
    fn end(&mut self, fallback: StopReason) -> Result<Vec<ModelEvent>, ApiError> {
        if std::mem::replace(&mut self.ended, true) {
            return Ok(vec![]);
        }
        Ok(vec![ModelEvent::Usage(self.tokens), ModelEvent::Stop(self.stop.clone().unwrap_or(fallback))])
    }

    /// The body ended. Without a stop event the reply was cut off (or was never a stream).
    fn end_of_body(&mut self) -> Result<Vec<ModelEvent>, ApiError> {
        if self.ended {
            return Ok(vec![]);
        }
        Err(ApiError::Retryable(CUT_OFF.into()))
    }
}

pub const CUT_OFF: &str = "The reply was cut off before it finished. Try again.";

pub fn tool_call(id: String, name: String, raw: &str) -> Result<ModelEvent, ApiError> {
    let args = if raw.trim().is_empty() { json!({}) } else { serde_json::from_str(raw).map_err(|_| ApiError::Retryable(format!("The model sent broken arguments for {name}.")))? };
    Ok(ModelEvent::ToolCall { id, name, args })
}

fn stop_reason(s: &str) -> StopReason {
    match s {
        "end_turn" | "stop_sequence" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        o => StopReason::Other(o.into()),
    }
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
pub(crate) mod tests;
