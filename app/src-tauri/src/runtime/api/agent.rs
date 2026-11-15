//! `ApiRuntime`: bleep's own tool loop. Tools are listed and called through bleep's own MCP
//! server, so the approval gate, remote connectors and untrusted wrapping work exactly as
//! they do for Claude Code. History is kept in SQLite in the neutral message form.
use super::adapter::ModelAdapter;
use super::types::*;
use crate::remote::client::client_info;
use crate::runtime::{preview, AgentEvent, AgentRuntime, EventStream, OneShot, RuntimeHealth, SessionConfig, SessionHandle, StaleResume, Tier, UserMessage};
use crate::store::api::ApiUsage;
use crate::store::Store;
use async_trait::async_trait;
use futures::StreamExt;
use rmcp::model::{CallToolRequestParams, ClientConfig};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

pub const MAX_STEPS: usize = 8;
const MAIN_MAX_TOKENS: u32 = 8192;
const ONE_SHOT_MAX_TOKENS: u32 = 4096;
const ONE_SHOT_TIMEOUT: Duration = Duration::from_secs(120);
/// A tool call can wait on an approval card, so it gets a long time.
const TOOL_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const ANSWER_TOOL: &str = "answer";
pub const KEY_REJECTED: &str = "The API key was rejected. Open Settings, then Model, to fix it.";

type Live = RunningService<RoleClient, ClientConfig>;

/// Which provider and models this runtime uses. The key is read from the Keychain by the caller.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiChoice {
    pub provider: String,
    pub label: String,
    pub main: String,
    pub small: String,
    pub has_key: bool,
    pub free: bool,
}

pub struct Core {
    choice: ApiChoice,
    adapter: Arc<dyn ModelAdapter>,
    store: Arc<Store>,
    models: RwLock<HashMap<String, ModelInfo>>,
    cfg: Mutex<Option<SessionConfig>>,
    mcp: tokio::sync::Mutex<Option<(String, Arc<Live>)>>,
    key_rejected: AtomicBool,
    retry_wait: Duration,
}

impl Core {
    pub fn new(choice: ApiChoice, adapter: Arc<dyn ModelAdapter>, store: Arc<Store>) -> Self {
        Self { choice, adapter, store, models: Default::default(), cfg: Mutex::new(None), mcp: Default::default(), key_rejected: AtomicBool::new(false), retry_wait: Duration::from_secs(2) }
    }

    pub fn with_retry_wait(mut self, d: Duration) -> Self {
        self.retry_wait = d;
        self
    }

    pub fn choice(&self) -> &ApiChoice {
        &self.choice
    }

    /// Fetches the provider's model list once, for context windows and prices.
    pub async fn load_models(&self) -> Result<Vec<ModelInfo>, ApiError> {
        let list = self.adapter.list_models().await?;
        *self.models.write().unwrap() = list.iter().map(|m| (m.id.clone(), m.clone())).collect();
        Ok(list)
    }

    fn model(&self, tier: Tier) -> &str {
        match tier {
            Tier::Main => &self.choice.main,
            Tier::Small => &self.choice.small,
        }
    }

    fn price(&self, model: &str) -> Option<Price> {
        if self.choice.free {
            return Some(Price::default());
        }
        self.models.read().unwrap().get(model).and_then(|m| m.price).or_else(|| super::prices::builtin(model))
    }

    fn window(&self, model: &str) -> Option<u64> {
        self.models.read().unwrap().get(model).and_then(|m| m.window)
    }

    fn record(&self, model: &str, purpose: &str, t: &Tokens) {
        let day = chrono::Local::now().date_naive().to_string();
        let cost = self.price(model).map(|p| p.cost(t));
        let _ = self.store.log_api_usage(&ApiUsage { day: &day, provider: &self.choice.provider, model, purpose, tokens_in: t.input, cache_read: t.cache_read, cache_write: t.cache_write, tokens_out: t.output, cost });
    }

    /// Starts a model call, trying once more after a short wait if it failed before any output.
    async fn open(&self, req: &ModelRequest) -> Result<ModelStream, ApiError> {
        match self.adapter.stream(req).await {
            Err(ApiError::Retryable(_)) => {
                tokio::time::sleep(self.retry_wait).await;
                self.adapter.stream(req).await
            }
            r => r,
        }
        .inspect_err(|e| {
            if matches!(e, ApiError::Auth(_)) {
                self.key_rejected.store(true, Ordering::Relaxed);
            }
        })
    }

    async fn peer(&self, url: &str, token: &str) -> Result<Arc<Live>, String> {
        let mut g = self.mcp.lock().await;
        let key = format!("{url} {token}");
        if let Some((k, live)) = g.as_ref() {
            if *k == key && !live.is_transport_closed() {
                return Ok(live.clone());
            }
        }
        let tc = StreamableHttpClientTransportConfig::with_uri(url.to_string()).auth_header(token.to_string());
        let live = match tokio::time::timeout(Duration::from_secs(20), client_info().serve(StreamableHttpClientTransport::from_config(tc))).await {
            Ok(r) => r.map_err(|e| format!("bleep's tool server did not answer: {e}"))?,
            Err(_) => return Err("bleep's tool server did not answer within 20 seconds.".into()),
        };
        let live = Arc::new(live);
        *g = Some((key, live.clone()));
        Ok(live)
    }

    async fn drop_peer(&self) {
        *self.mcp.lock().await = None;
    }

    async fn tools(&self, cfg: &SessionConfig) -> Result<Vec<ToolDef>, String> {
        let mut last = String::new();
        for _ in 0..2 {
            let peer = self.peer(&cfg.mcp.url, &cfg.mcp.token).await?;
            match tokio::time::timeout(Duration::from_secs(20), peer.list_all_tools()).await {
                Ok(Ok(list)) => {
                    return Ok(list
                        .into_iter()
                        .map(|t| ToolDef { name: t.name.to_string(), description: t.description.map(|d| d.to_string()).unwrap_or_default(), schema: Value::Object((*t.input_schema).clone()) })
                        .collect())
                }
                Ok(Err(e)) => last = e.to_string(),
                Err(_) => last = "it took over 20 seconds".into(),
            }
            self.drop_peer().await;
        }
        Err(format!("bleep's tools could not be listed: {last}"))
    }

    /// One `tools/call` on bleep's server. Never sent twice: a write must not run twice.
    async fn call(&self, cfg: &SessionConfig, name: &str, args: &Value) -> (bool, String) {
        let peer = match self.peer(&cfg.mcp.url, &cfg.mcp.token).await {
            Ok(p) => p,
            Err(e) => return (false, e),
        };
        let mut params = CallToolRequestParams::new(name.to_string());
        if let Value::Object(o) = args {
            params = params.with_arguments(o.clone());
        }
        match tokio::time::timeout(TOOL_TIMEOUT, peer.call_tool(params)).await {
            Ok(Ok(r)) => {
                let v = serde_json::to_value(&r).unwrap_or_default();
                let mut text: Vec<String> = v["content"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str().map(str::to_string)).collect();
                if text.is_empty() && !v["structuredContent"].is_null() {
                    text.push(v["structuredContent"].to_string());
                }
                (!v["isError"].as_bool().unwrap_or(false), text.join("\n"))
            }
            Ok(Err(e)) => {
                self.drop_peer().await;
                (false, format!("The tool call failed: {e}"))
            }
            Err(_) => {
                self.drop_peer().await;
                (false, "The tool did not finish in time.".into())
            }
        }
    }

    fn history(&self, conv: &str) -> Vec<Msg> {
        let raw = self.store.api_messages(conv).unwrap_or_default();
        sanitize(raw.iter().filter_map(|s| serde_json::from_str(s).ok()).collect())
    }

    fn save(&self, conv: &str, m: &Msg) {
        if let Ok(s) = serde_json::to_string(m) {
            let _ = self.store.api_append(conv, &s);
        }
    }

    async fn turn(self: Arc<Self>, cfg: SessionConfig, conv: String, text: String, tx: tokio::sync::mpsc::Sender<AgentEvent>) {
        let out = |ev: AgentEvent| {
            let tx = tx.clone();
            async move { tx.send(ev).await.is_ok() }
        };
        let user = Msg::user_text(&text);
        self.save(&conv, &user);
        let mut msgs = self.history(&conv);
        let tools = match self.tools(&cfg).await {
            Ok(t) => t,
            Err(e) => {
                out(AgentEvent::Error { message: e, retryable: true }).await;
                out(AgentEvent::Done { runtime_session_id: Some(conv) }).await;
                return;
            }
        };
        let model = self.model(cfg.tier).to_string();
        let mut total = Tokens::default();
        let mut last_input = 0;
        for step in 0..=MAX_STEPS {
            let req = ModelRequest { model: model.clone(), system: cfg.system_prompt.clone(), msgs: msgs.clone(), tools: tools.clone(), choice: if step == MAX_STEPS { ToolChoice::None } else { ToolChoice::Auto }, max_tokens: MAIN_MAX_TOKENS };
            let mut stream = match self.open(&req).await {
                Ok(s) => s,
                Err(e) => return self.fail(e, &conv, total, &out).await,
            };
            let (mut said, mut calls, mut failed) = (String::new(), vec![], None);
            while let Some(ev) = stream.next().await {
                match ev {
                    Ok(ModelEvent::TextDelta(t)) => {
                        said.push_str(&t);
                        if !out(AgentEvent::TextDelta { text: t }).await {
                            return;
                        }
                    }
                    Ok(ModelEvent::ToolCall { id, name, args }) => calls.push((id, name, args)),
                    Ok(ModelEvent::Usage(t)) => {
                        self.record(&model, "chat", &t);
                        total.input += t.input;
                        total.cache_read += t.cache_read;
                        total.cache_write += t.cache_write;
                        total.output += t.output;
                        last_input = t.all_input();
                    }
                    Ok(ModelEvent::Stop(_)) => {}
                    Err(e) => {
                        failed = Some(e);
                        break;
                    }
                }
            }
            let mut parts = vec![];
            if !said.is_empty() {
                parts.push(Part::Text { text: said });
            }
            if failed.is_some() || step == MAX_STEPS {
                // A cut-off reply keeps its text but not half-made tool calls, and the last step runs none.
                calls.clear();
            }
            parts.extend(calls.iter().map(|(id, name, args)| Part::ToolCall { id: id.clone(), name: name.clone(), args: args.clone() }));
            if !parts.is_empty() {
                let m = Msg { role: Role::Assistant, parts };
                self.save(&conv, &m);
                msgs.push(m);
            }
            if let Some(e) = failed {
                return self.fail(e, &conv, total, &out).await;
            }
            // On the last step the model may not use tools; any it asks for anyway are not run.
            if calls.is_empty() || step == MAX_STEPS {
                break;
            }
            let mut results = vec![];
            let mut stopped = false;
            for (id, name, args) in calls {
                // Stopped (the chat dropped the stream): the rest are not run, and what ran is kept.
                if stopped || !out(AgentEvent::ToolStart { id: id.clone(), name: name.clone(), args: args.clone() }).await {
                    stopped = true;
                    results.push(Part::ToolResult { id, ok: false, text: "Not run: the turn was stopped.".into() });
                    continue;
                }
                let (ok, text) = self.call(&cfg, &name, &args).await;
                out(AgentEvent::ToolResult { id: id.clone(), ok, preview: preview(&text, 200) }).await;
                results.push(Part::ToolResult { id, ok, text });
            }
            let m = Msg { role: Role::User, parts: results };
            self.save(&conv, &m);
            if stopped {
                return;
            }
            msgs.push(m);
        }
        out(AgentEvent::Usage { input: total.all_input(), output: total.output }).await;
        out(AgentEvent::Context { used: last_input, window: self.window(&model) }).await;
        out(AgentEvent::Done { runtime_session_id: Some(conv) }).await;
    }

    async fn fail<F, Fut>(&self, e: ApiError, conv: &str, total: Tokens, out: &F)
    where
        F: Fn(AgentEvent) -> Fut,
        Fut: std::future::Future<Output = bool>,
    {
        if total.output > 0 {
            out(AgentEvent::Usage { input: total.all_input(), output: total.output }).await;
        }
        let (message, retryable) = match &e {
            ApiError::RateLimited { until } => {
                out(AgentEvent::RateLimited { until: *until }).await;
                let at = chrono::DateTime::from_timestamp(*until, 0).map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string()).unwrap_or_default();
                (format!("The provider's rate limit was hit. Try again after {at}."), true)
            }
            ApiError::Auth(d) => (format!("{KEY_REJECTED} {d}"), false),
            ApiError::Retryable(d) => (d.clone(), true),
            ApiError::Fatal(d) => (d.clone(), false),
        };
        out(AgentEvent::Error { message, retryable }).await;
        out(AgentEvent::Done { runtime_session_id: Some(conv.to_string()) }).await;
    }
}

/// Makes stored history valid for any provider: starts with the user, roles alternate, and
/// every tool call has a result (a turn that was stopped gets "not run" results).
pub fn sanitize(msgs: Vec<Msg>) -> Vec<Msg> {
    let mut out: Vec<Msg> = vec![];
    for m in msgs.into_iter().skip_while(|m| m.role == Role::Assistant) {
        if let Some(prev) = out.last() {
            let open: Vec<String> = pending_calls(prev, &m);
            if !open.is_empty() {
                let parts = open.into_iter().map(|id| Part::ToolResult { id, ok: false, text: "Not run: the turn was stopped.".into() }).collect();
                out.push(Msg { role: Role::User, parts });
            }
        }
        match out.last_mut() {
            Some(prev) if prev.role == m.role => {
                // Tool results go first in a user message.
                let (mut res, rest): (Vec<Part>, Vec<Part>) = m.parts.into_iter().partition(|p| matches!(p, Part::ToolResult { .. }));
                if m.role == Role::User && !res.is_empty() {
                    let pos = prev.parts.iter().take_while(|p| matches!(p, Part::ToolResult { .. })).count();
                    prev.parts.splice(pos..pos, res.drain(..));
                }
                prev.parts.extend(rest);
            }
            _ => out.push(m),
        }
    }
    if let Some(last) = out.last() {
        let open = pending_calls(last, &Msg { role: Role::Assistant, parts: vec![] });
        if !open.is_empty() {
            out.push(Msg { role: Role::User, parts: open.into_iter().map(|id| Part::ToolResult { id, ok: false, text: "Not run: the turn was stopped.".into() }).collect() });
        }
    }
    out
}

/// Tool call ids in `prev` (an assistant message) that `next` does not answer.
fn pending_calls(prev: &Msg, next: &Msg) -> Vec<String> {
    if prev.role != Role::Assistant {
        return vec![];
    }
    let answered: Vec<&str> = if next.role == Role::User { next.parts.iter().filter_map(|p| if let Part::ToolResult { id, .. } = p { Some(id.as_str()) } else { None }).collect() } else { vec![] };
    prev.parts.iter().filter_map(|p| if let Part::ToolCall { id, .. } = p { (!answered.contains(&id.as_str())).then(|| id.clone()) } else { None }).collect()
}

fn new_conv() -> String {
    format!("api-{}", &crate::mcp::new_token()[..24])
}

/// The runtime handle. Cheap to clone; a turn runs on its own task.
#[derive(Clone)]
pub struct ApiRuntime(pub Arc<Core>);

impl ApiRuntime {
    pub fn new(core: Core) -> Self {
        Self(Arc::new(core))
    }
}

impl std::ops::Deref for ApiRuntime {
    type Target = Core;
    fn deref(&self) -> &Core {
        &self.0
    }
}

#[async_trait]
impl AgentRuntime for ApiRuntime {
    fn id(&self) -> &'static str {
        "api"
    }

    async fn health(&self) -> RuntimeHealth {
        let c = &self.choice;
        let rejected = self.key_rejected.load(Ordering::Relaxed);
        let signed_in = c.has_key && !rejected && !c.main.is_empty() && !c.small.is_empty();
        let hint = if !c.has_key {
            Some(format!("Add your {} key in Settings, then Model.", c.label))
        } else if rejected {
            Some(KEY_REJECTED.into())
        } else if c.main.is_empty() || c.small.is_empty() {
            Some("Pick a main and a small model in Settings, then Model.".into())
        } else {
            None
        };
        RuntimeHealth { runtime: "api".into(), installed: true, version: Some(c.main.clone()), version_ok: true, signed_in, plan: Some(c.label.clone()), hint }
    }

    async fn start_session(&self, cfg: SessionConfig) -> anyhow::Result<SessionHandle> {
        let conv = match &cfg.resume_id {
            Some(id) if self.store.api_conv_exists(id)? => id.clone(),
            Some(id) => return Err(StaleResume(format!("No saved conversation {id}.")).into()),
            None => new_conv(),
        };
        *self.cfg.lock().unwrap() = Some(cfg);
        Ok(SessionHandle { resume_id: Some(conv) })
    }

    async fn send(&self, s: &SessionHandle, msg: UserMessage) -> anyhow::Result<EventStream> {
        let cfg = self.cfg.lock().unwrap().clone().ok_or_else(|| anyhow::anyhow!("No session was started."))?;
        let conv = s.resume_id.clone().unwrap_or_else(new_conv);
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        tokio::spawn(self.0.clone().turn(cfg, conv, msg.text, tx));
        Ok(Box::pin(futures::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|e| (e, rx)) })))
    }

    async fn one_shot(&self, req: OneShot) -> anyhow::Result<Value> {
        let model = self.model(req.tier).to_string();
        let mut parts = vec![Part::Text { text: req.prompt }];
        parts.extend(req.images.into_iter().map(|i| Part::Image { media_type: i.media_type, data_b64: i.data_b64 }));
        let schema = if req.schema.is_object() && req.schema.get("type").is_some() { req.schema } else { json!({"type": "object"}) };
        let mreq = ModelRequest {
            model: model.clone(),
            system: String::new(),
            msgs: vec![Msg { role: Role::User, parts }],
            tools: vec![ToolDef { name: ANSWER_TOOL.into(), description: "Give your answer in this shape.".into(), schema }],
            choice: ToolChoice::Force(ANSWER_TOOL.into()),
            max_tokens: ONE_SHOT_MAX_TOKENS,
        };
        let run = async {
            let mut stream = self.open(&mreq).await?;
            let (mut text, mut answer) = (String::new(), None);
            while let Some(ev) = stream.next().await {
                match ev? {
                    ModelEvent::TextDelta(t) => text.push_str(&t),
                    ModelEvent::ToolCall { name, args, .. } if name == ANSWER_TOOL && answer.is_none() => answer = Some(args),
                    ModelEvent::Usage(t) => self.record(&model, "one_shot", &t),
                    _ => {}
                }
            }
            Ok::<_, ApiError>(answer.or_else(|| serde_json::from_str(text.trim().trim_start_matches("```json").trim_matches('`').trim()).ok()))
        };
        match tokio::time::timeout(ONE_SHOT_TIMEOUT, run).await {
            Ok(Ok(Some(v))) => Ok(v),
            Ok(Ok(None)) => Err(anyhow::anyhow!("The model gave no answer in the expected shape.")),
            Ok(Err(e)) => Err(anyhow::anyhow!("{e}")),
            Err(_) => Err(anyhow::anyhow!("The model did not answer within 2 minutes.")),
        }
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
