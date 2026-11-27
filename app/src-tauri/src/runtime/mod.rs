//! The agent runtime boundary. Chat, scheduler and compaction only see this trait,
//! never which vendor runs. Nothing here assumes a child process or a CLI session.
pub mod api;
pub mod claude;
pub mod switch;
pub mod contract;
pub mod fake;

use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use serde::Serialize;
use serde_json::Value;
use std::pin::Pin;

#[async_trait]
pub trait AgentRuntime: Send + Sync {
    fn id(&self) -> &'static str;
    /// Changes when the runtime behind this one is rebuilt (a Settings save), so the chat
    /// knows to start its session again on the new one.
    fn instance(&self) -> u64 {
        0
    }
    async fn health(&self) -> RuntimeHealth;
    async fn start_session(&self, cfg: SessionConfig) -> Result<SessionHandle>;
    async fn send(&self, s: &SessionHandle, msg: UserMessage) -> Result<EventStream>;
    async fn one_shot(&self, req: OneShot) -> Result<Value>;
    /// A one-shot plus the exact tokens the backend reported for it. Backends that report
    /// none give `None`.
    async fn one_shot_with_usage(&self, req: OneShot) -> Result<(Value, Option<ShotUsage>)> {
        Ok((self.one_shot(req).await?, None))
    }
}

/// Tokens of one call as the provider counted them. `input` includes cache reads and writes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShotUsage {
    pub input: u64,
    pub output: u64,
}

/// Runs a background one-shot and logs it with the exact tokens the backend reported.
pub async fn logged_one_shot(rt: &dyn AgentRuntime, store: &crate::store::Store, purpose: &str, req: OneShot) -> Result<Value> {
    let tier = match req.tier {
        Tier::Main => "main",
        Tier::Small => "small",
    };
    let r = rt.one_shot_with_usage(req).await;
    let u = r.as_ref().ok().and_then(|(_, u)| *u);
    let _ = store.log_llm(rt.id(), tier, purpose, u.map(|u| u.input as i64), u.map(|u| u.output as i64), r.is_ok());
    r.map(|(v, _)| v)
}

pub type EventStream = Pin<Box<dyn Stream<Item = AgentEvent> + Send>>;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    TextDelta { text: String },
    ToolStart { id: String, name: String, args: Value },
    /// `preview` is at most 200 chars.
    ToolResult { id: String, ok: bool, preview: String },
    /// `input` = input + cache_creation + cache_read tokens.
    Usage { input: u64, output: u64 },
    /// How full the session is after this turn: the last model call's input, and the
    /// model's context window when the runtime reports it.
    Context { used: u64, window: Option<u64> },
    /// Unix seconds when the limit resets.
    RateLimited { until: i64 },
    Done { runtime_session_id: Option<String> },
    Error { message: String, retryable: bool },
}

#[derive(Clone, Debug)]
pub struct McpEndpoint {
    pub url: String,
    pub token: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tier {
    Main,
    Small,
}

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub system_prompt: String,
    pub mcp: McpEndpoint,
    pub tier: Tier,
    pub resume_id: Option<String>,
}

/// The backend keeps its own state keyed by this.
#[derive(Clone, Debug)]
pub struct SessionHandle {
    pub resume_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct UserMessage {
    pub text: String,
}

/// An image for a one-shot, as base64 (a screenshot from an agent transcript).
#[derive(Clone, Debug, PartialEq)]
pub struct ImageIn {
    pub media_type: String,
    pub data_b64: String,
}

#[derive(Clone, Debug)]
pub struct OneShot {
    pub prompt: String,
    pub schema: Value,
    pub tier: Tier,
    /// Sent with the prompt when not empty.
    pub images: Vec<ImageIn>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct RuntimeHealth {
    pub runtime: String,
    pub installed: bool,
    pub version: Option<String>,
    pub version_ok: bool,
    pub signed_in: bool,
    pub plan: Option<String>,
    pub hint: Option<String>,
}

impl RuntimeHealth {
    pub fn ok(&self) -> bool {
        self.installed && self.version_ok && self.signed_in
    }
}

/// The runtime could not resume this session id (for example its file is gone).
/// Only this error makes the chat start a fresh session.
#[derive(Debug)]
pub struct StaleResume(pub String);

impl std::fmt::Display for StaleResume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for StaleResume {}

/// Cut a tool result to at most `max` chars on a char boundary.
pub fn preview(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}
