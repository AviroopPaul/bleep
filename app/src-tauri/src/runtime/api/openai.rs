//! OpenAI Chat Completions with streaming. One adapter for OpenAI, OpenRouter, Gemini's
//! OpenAI-compatible endpoint, Groq, Ollama, LM Studio and any other compatible server.
use super::adapter::*;
use super::anthropic::tool_call;
use super::types::*;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub struct OpenAiAdapter {
    pub base_url: String,
    pub key: String,
    pub max_completion_tokens: bool,
    pub http: reqwest::Client,
}

impl OpenAiAdapter {
    pub fn new(base_url: &str, key: &str, max_completion_tokens: bool) -> Self {
        Self { base_url: base_url.trim_end_matches('/').to_string(), key: key.to_string(), max_completion_tokens, http: http() }
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let r = self.http.request(method, format!("{}/{path}", self.base_url)).header("X-Title", "bleep");
        if self.key.is_empty() { r } else { r.bearer_auth(&self.key) }
    }
}

#[async_trait]
impl ModelAdapter for OpenAiAdapter {
    async fn stream(&self, req: &ModelRequest) -> Result<ModelStream, ApiError> {
        let resp = send(self.req(reqwest::Method::POST, "chat/completions").json(&body(req, self.max_completion_tokens))).await?;
        Ok(drive(resp, Parser::default()))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ApiError> {
        let resp = send(self.req(reqwest::Method::GET, "models")).await?;
        let v: Value = resp.json().await.map_err(|e| ApiError::Fatal(format!("The model list could not be read: {e}")))?;
        Ok(parse_models(&v))
    }
}

/// Per-token price strings (OpenRouter) to dollars per million tokens.
fn per_m(v: &Value) -> Option<f64> {
    let n = v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64())?;
    (n >= 0.0).then_some(n * 1_000_000.0)
}

pub fn parse_models(v: &Value) -> Vec<ModelInfo> {
    let list = v["data"].as_array().or_else(|| v["models"].as_array());
    let mut out: Vec<ModelInfo> = list
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    let id = m["id"].as_str()?.trim_start_matches("models/").to_string();
                    let p = &m["pricing"];
                    let price = match (per_m(&p["prompt"]), per_m(&p["completion"])) {
                        (Some(input), Some(output)) => Some(Price { input, output, cache_read: per_m(&p["input_cache_read"]).unwrap_or(input), cache_write: per_m(&p["input_cache_write"]).unwrap_or(input) }),
                        _ => super::prices::builtin(&id),
                    };
                    let window = m["context_length"].as_u64().or_else(|| m["context_window"].as_u64()).or_else(|| m["inputTokenLimit"].as_u64());
                    Some(ModelInfo { name: m["name"].as_str().or(m["display_name"].as_str()).map(str::to_string), window, price, id })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn body(r: &ModelRequest, max_completion_tokens: bool) -> Value {
    let mut msgs = vec![];
    if !r.system.is_empty() {
        msgs.push(json!({"role": "system", "content": r.system}));
    }
    for m in &r.msgs {
        messages(m, &mut msgs);
    }
    let mut b = json!({"model": r.model, "messages": msgs, "stream": true, "stream_options": {"include_usage": true}});
    b[if max_completion_tokens { "max_completion_tokens" } else { "max_tokens" }] = json!(r.max_tokens);
    if !r.tools.is_empty() {
        b["tools"] = r.tools.iter().map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.schema}})).collect();
        b["tool_choice"] = match &r.choice {
            ToolChoice::Auto => json!("auto"),
            ToolChoice::None => json!("none"),
            ToolChoice::Force(n) => json!({"type": "function", "function": {"name": n}}),
        };
    }
    b
}

fn messages(m: &Msg, out: &mut Vec<Value>) {
    match m.role {
        Role::Assistant => {
            let text: String = m.parts.iter().filter_map(|p| if let Part::Text { text } = p { Some(text.as_str()) } else { None }).collect();
            let calls: Vec<Value> = m
                .parts
                .iter()
                .filter_map(|p| match p {
                    Part::ToolCall { id, name, args } => Some(json!({"id": id, "type": "function", "function": {"name": name, "arguments": args.to_string()}})),
                    _ => None,
                })
                .collect();
            let mut a = json!({"role": "assistant", "content": if text.is_empty() { Value::Null } else { json!(text) }});
            if !calls.is_empty() {
                a["tool_calls"] = Value::Array(calls);
            }
            out.push(a);
        }
        Role::User => {
            // Tool results are their own messages and must come right after the assistant's calls.
            for p in &m.parts {
                if let Part::ToolResult { id, ok, text } = p {
                    let content = if *ok { text.clone() } else { format!("Error: {text}") };
                    out.push(json!({"role": "tool", "tool_call_id": id, "content": content}));
                }
            }
            let rest: Vec<&Part> = m.parts.iter().filter(|p| !matches!(p, Part::ToolResult { .. } | Part::ToolCall { .. })).collect();
            if rest.is_empty() {
                return;
            }
            let content = if rest.iter().all(|p| matches!(p, Part::Text { .. })) {
                json!(rest.iter().filter_map(|p| if let Part::Text { text } = p { Some(text.as_str()) } else { None }).collect::<Vec<_>>().join("\n"))
            } else {
                rest.iter()
                    .map(|p| match p {
                        Part::Image { media_type, data_b64 } => json!({"type": "image_url", "image_url": {"url": format!("data:{media_type};base64,{data_b64}")}}),
                        Part::Text { text } => json!({"type": "text", "text": text}),
                        _ => unreachable!(),
                    })
                    .collect()
            };
            out.push(json!({"role": "user", "content": content}));
        }
    }
}

#[derive(Default)]
pub struct Parser {
    tokens: Tokens,
    stop: Option<StopReason>,
    /// Tool calls by index: id, name, streamed arguments.
    tools: BTreeMap<u64, (String, String, String)>,
    ended: bool,
}

impl WireParser for Parser {
    fn on(&mut self, ev: &SseEvent) -> Result<Vec<ModelEvent>, ApiError> {
        if ev.data.trim() == "[DONE]" {
            return self.end();
        }
        let v: Value = match serde_json::from_str(&ev.data) {
            Ok(v) => v,
            Err(_) => return Ok(vec![]),
        };
        if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
            let msg = format!("The provider stopped with an error {}", server_text(&error_message(&json!({"error": e}).to_string())));
            let code = e["code"].as_u64().or_else(|| e["code"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0) as u16;
            return Err(match code {
                429 => ApiError::RateLimited { until: chrono::Utc::now().timestamp() + 60 },
                401 | 403 => ApiError::Auth(msg),
                0 | 408 | 500..=599 => ApiError::Retryable(msg),
                _ => ApiError::Fatal(msg),
            });
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            let prompt = u["prompt_tokens"].as_u64().unwrap_or(0);
            let cached = u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0).min(prompt);
            self.tokens = Tokens { input: prompt - cached, cache_read: cached, cache_write: 0, output: u["completion_tokens"].as_u64().unwrap_or(0) };
        }
        let mut out = vec![];
        let Some(c) = v["choices"].get(0) else { return Ok(out) };
        let d = &c["delta"];
        if let Some(t) = d["content"].as_str().filter(|t| !t.is_empty()) {
            out.push(ModelEvent::TextDelta(t.into()));
        }
        for tc in d["tool_calls"].as_array().into_iter().flatten() {
            let idx = tc["index"].as_u64().unwrap_or(self.tools.len() as u64);
            let e = self.tools.entry(idx).or_default();
            if let Some(id) = tc["id"].as_str().filter(|s| !s.is_empty()) {
                e.0 = id.into();
            }
            if let Some(n) = tc["function"]["name"].as_str().filter(|s| !s.is_empty()) {
                e.1 = n.into();
            }
            e.2.push_str(tc["function"]["arguments"].as_str().unwrap_or(""));
        }
        if let Some(f) = c["finish_reason"].as_str() {
            self.stop = Some(match f {
                "stop" => StopReason::EndTurn,
                "tool_calls" | "function_call" => StopReason::ToolUse,
                "length" => StopReason::MaxTokens,
                o => StopReason::Other(o.into()),
            });
        }
        Ok(out)
    }

    fn finish(&mut self) -> Vec<ModelEvent> {
        self.end().unwrap_or_default()
    }
}

impl Parser {
    fn end(&mut self) -> Result<Vec<ModelEvent>, ApiError> {
        if std::mem::replace(&mut self.ended, true) {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for (idx, (id, name, raw)) in std::mem::take(&mut self.tools) {
            let id = if id.is_empty() { format!("call_{idx}") } else { id };
            out.push(tool_call(id, name, &raw)?);
        }
        // Some servers say "stop" even when they called tools.
        let stop = match self.stop.clone() {
            Some(StopReason::EndTurn) if !out.is_empty() => StopReason::ToolUse,
            Some(s) => s,
            None => StopReason::Other("ended early".into()),
        };
        out.push(ModelEvent::Usage(self.tokens));
        out.push(ModelEvent::Stop(stop));
        Ok(out)
    }
}

#[cfg(test)]
#[path = "openai_tests.rs"]
mod tests;
