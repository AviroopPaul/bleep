//! Memory tools (build plan 8.2 and 8.7): search is read, a daily note is safe, and a fact
//! for the always-on memory files is a write that waits for the user's yes.
use super::{Connector, ConnectorStatus, Risk, ToolSpec};
use crate::memory::{search, Added, Memory};
use crate::paths::MemFile;
use async_trait::async_trait;
use chrono::{DateTime, Local};
use serde_json::{json, Value};
use std::sync::Arc;

pub type Clock = Arc<dyn Fn() -> DateTime<Local> + Send + Sync>;

pub struct MemoryConnector {
    mem: Arc<Memory>,
    now: Clock,
}

impl MemoryConnector {
    pub fn new(mem: Arc<Memory>, now: Clock) -> MemoryConnector {
        MemoryConnector { mem, now }
    }
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| format!("{key} is required and must be text."))
}

fn file_arg(args: &Value) -> Result<MemFile, String> {
    match args.get("file").and_then(Value::as_str) {
        None => Ok(MemFile::Memory),
        Some(s) => MemFile::from_name(s).ok_or_else(|| "file must be USER.md, RULES.md or MEMORY.md.".to_string()),
    }
}

#[async_trait]
impl Connector for MemoryConnector {
    fn id(&self) -> &'static str {
        "memory"
    }

    fn tools(&self) -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "memory_search".into(),
                description: "Search your daily notes and the earlier chat. Use it when the user says earlier, last week, or what did I say about, and before you say you do not know something they may have told you.".into(),
                schema: json!({"type": "object", "properties": {"query": {"type": "string", "description": "A few key words"}}, "required": ["query"]}),
                risk: Risk::Read,
            },
            ToolSpec {
                name: "memory_note".into(),
                description: "Save one short line to today's daily note: something worth keeping that is not a lasting fact. Notes are found later with memory_search.".into(),
                schema: json!({"type": "object", "properties": {"text": {"type": "string", "description": "One line, at most 300 characters"}}, "required": ["text"]}),
                risk: Risk::Safe,
            },
            ToolSpec {
                name: "memory_promote".into(),
                description: "Add a lasting fact or preference the user stated to the memory you always see. The user gets a card with the exact fact and must approve it. One fact, one line. USER.md is about the user and their people, RULES.md is how you should behave, MEMORY.md is everything else.".into(),
                schema: json!({"type": "object", "properties": {
                    "fact": {"type": "string", "description": "One line, at most 300 characters"},
                    "file": {"type": "string", "enum": ["MEMORY.md", "USER.md", "RULES.md"]}
                }, "required": ["fact"]}),
                risk: Risk::Write,
            },
        ]
    }

    fn status(&self) -> ConnectorStatus {
        ConnectorStatus::Connected { account: "this Mac".into() }
    }

    async fn call(&self, tool: &str, args: Value) -> Result<Value, String> {
        match tool {
            "memory_search" => {
                let q = text_arg(&args, "query")?;
                let hits = search::search(&self.mem.store, q).map_err(|e| e.to_string())?;
                if hits.is_empty() {
                    return Ok(json!({"hits": [], "note": "No matches in notes or earlier chat."}));
                }
                Ok(json!({"hits": hits}))
            }
            "memory_note" => {
                let t = text_arg(&args, "text")?;
                self.mem.append_note((self.now)(), t)?;
                Ok(json!({"saved": true}))
            }
            _ => Err(format!("Unknown tool: {tool}")),
        }
    }

    /// The card shows exactly what will be saved, and only a fact that can be saved.
    async fn prepare(&self, tool: &str, args: Value) -> Result<Value, String> {
        if tool != "memory_promote" {
            return Err(format!("Unknown tool: {tool}"));
        }
        let file = file_arg(&args)?;
        let fact = self.mem.check_fact(text_arg(&args, "fact")?)?;
        if self.mem.can_add(&fact)? == Added::Duplicate {
            return Err("That is already in memory. Nothing to add.".into());
        }
        Ok(json!({"fact": fact, "file": file.file_name()}))
    }

    async fn execute(&self, tool: &str, payload: Value) -> Result<Value, String> {
        if tool != "memory_promote" {
            return Err(format!("{tool} cannot change anything"));
        }
        let file = file_arg(&payload)?;
        match self.mem.add_fact(file, text_arg(&payload, "fact")?)? {
            Added::Yes => Ok(json!({"saved": true})),
            Added::Duplicate => Ok(json!({"saved": false, "reason": "Already in memory."})),
        }
    }
}

#[cfg(test)]
mod tests;
