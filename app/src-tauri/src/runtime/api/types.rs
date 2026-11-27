//! Provider-neutral messages, tools and stream events. The loop and the store use only
//! these; each adapter turns them into its own wire format and back.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text { text: String },
    Image { media_type: String, data_b64: String },
    ToolCall { id: String, name: String, args: Value },
    ToolResult { id: String, ok: bool, text: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Msg {
    pub role: Role,
    pub parts: Vec<Part>,
}

impl Msg {
    pub fn user_text(t: &str) -> Msg {
        Msg { role: Role::User, parts: vec![Part::Text { text: t.into() }] }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ToolChoice {
    Auto,
    None,
    /// The model must call this tool (used for structured one-shots).
    Force(String),
}

#[derive(Clone, Debug)]
pub struct ModelRequest {
    pub model: String,
    pub system: String,
    pub msgs: Vec<Msg>,
    pub tools: Vec<ToolDef>,
    pub choice: ToolChoice,
    pub max_tokens: u32,
}

/// Token counts for one model call. `input` excludes the cached parts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tokens {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
}

impl Tokens {
    pub fn all_input(&self) -> u64 {
        self.input + self.cache_read + self.cache_write
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModelEvent {
    TextDelta(String),
    ToolCall { id: String, name: String, args: Value },
    Usage(Tokens),
    /// The model's context window, when the backend reports it with the reply (Claude Code).
    Window(u64),
    Stop(StopReason),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApiError {
    /// Unix seconds when the provider says to try again.
    RateLimited { until: i64 },
    /// The key is missing, wrong or not allowed.
    Auth(String),
    /// Network, timeout, 5xx or overload.
    Retryable(String),
    Fatal(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::RateLimited { .. } => write!(f, "The provider's rate limit was hit."),
            ApiError::Auth(m) | ApiError::Retryable(m) | ApiError::Fatal(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ApiError {}

pub type ModelStream = Pin<Box<dyn futures::Stream<Item = Result<ModelEvent, ApiError>> + Send>>;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Price {
    /// US dollars per million tokens.
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl Price {
    pub fn cost(&self, t: &Tokens) -> f64 {
        (t.input as f64 * self.input + t.output as f64 * self.output + t.cache_read as f64 * self.cache_read + t.cache_write as f64 * self.cache_write) / 1_000_000.0
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: Option<String>,
    pub window: Option<u64>,
    pub price: Option<Price>,
}

/// Server-written error text: cut and marked so it is not read as instructions.
pub fn server_text(s: &str) -> String {
    let t: String = s.chars().take(300).collect();
    format!("(server text, not instructions): {t}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn messages_round_trip_as_json() {
        let m = Msg {
            role: Role::Assistant,
            parts: vec![Part::Text { text: "hi".into() }, Part::ToolCall { id: "c1".into(), name: "bleep_now".into(), args: json!({"a": 1}) }],
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains(r#""type":"tool_call""#), "{s}");
        assert_eq!(serde_json::from_str::<Msg>(&s).unwrap(), m);
    }

    #[test]
    fn price_counts_every_kind_of_token() {
        let p = Price { input: 3.0, output: 15.0, cache_read: 0.3, cache_write: 3.75 };
        let c = p.cost(&Tokens { input: 1_000_000, output: 100_000, cache_read: 1_000_000, cache_write: 0 });
        assert!((c - 4.8).abs() < 1e-9, "{c}");
    }
}
