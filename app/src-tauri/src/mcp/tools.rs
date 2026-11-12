//! bleep's own tools. Every tool has a risk level; the gate decides, not the model (build plan 8.2).
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Risk {
    /// Runs at once.
    Read,
    /// Runs at once; nothing leaves the account and it is easy to undo.
    Safe,
    /// Needs an approval card (Phase 2).
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

pub fn registry() -> Vec<ToolSpec> {
    vec![ToolSpec {
        name: "bleep_now",
        description: "The current local date, time, weekday and time zone on this Mac. Use it whenever the answer depends on the time.",
        schema: json!({"type": "object", "properties": {}}),
        risk: Risk::Read,
    }]
}

/// Phase 1 gate: read and safe tools run; write tools wait for the approval cards in Phase 2.
pub fn gate(spec: &ToolSpec) -> Result<(), &'static str> {
    match spec.risk {
        Risk::Read | Risk::Safe => Ok(()),
        Risk::Write | Risk::Never => Err("Not available yet."),
    }
}

pub fn bleep_now() -> Value {
    let now = chrono::Local::now();
    json!({
        "iso": now.to_rfc3339(),
        "date": now.format("%Y-%m-%d").to_string(),
        "time": now.format("%H:%M").to_string(),
        "weekday": now.format("%A").to_string(),
        "timezone": iana_time_zone::get_timezone().unwrap_or_else(|_| "unknown".into()),
        "utc_offset": now.format("%:z").to_string(),
    })
}

/// Run a tool that passed the gate.
pub fn run(name: &str, _args: &Value) -> Result<Value, String> {
    match name {
        "bleep_now" => Ok(bleep_now()),
        _ => Err(format!("Unknown tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_tools_are_not_available_yet() {
        let spec = ToolSpec { name: "x_write", description: "", schema: serde_json::json!({"type": "object"}), risk: Risk::Write };
        assert_eq!(gate(&spec), Err("Not available yet."));
        assert_eq!(gate(&registry()[0]), Ok(()));
    }

    #[test]
    fn tool_names_are_snake_case() {
        for t in registry() {
            assert!(t.name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{}", t.name);
        }
    }
}
