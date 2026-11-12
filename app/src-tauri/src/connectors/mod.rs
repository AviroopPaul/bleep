//! Data sources that give bleep tools (build plan 8.3).
use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ConnectorStatus {
    Connected { account: String },
    NeedsSetup,
    NeedsSignIn,
    Error { message: String },
}

pub mod bleep;
pub mod calendar;
pub mod gmail;

use async_trait::async_trait;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Risk {
    /// Runs at once.
    Read,
    /// Runs at once; nothing leaves the account and it is easy to undo.
    Safe,
    /// Waits for an approval card.
    Write,
    /// Never built.
    Never,
}

#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
    pub risk: Risk,
}

/// A source of tools. Read and safe tools go through `call`; write tools go through
/// `prepare` (the exact payload the card shows) and, after approval, `execute` with that payload.
#[async_trait]
pub trait Connector: Send + Sync {
    fn id(&self) -> &'static str;
    fn tools(&self) -> Vec<ToolSpec>;
    fn status(&self) -> ConnectorStatus;
    async fn call(&self, tool: &str, args: Value) -> Result<Value, String>;
    async fn prepare(&self, _tool: &str, args: Value) -> Result<Value, String> {
        Ok(args)
    }
    async fn execute(&self, tool: &str, _payload: Value) -> Result<Value, String> {
        Err(format!("{tool} cannot change anything"))
    }
}
