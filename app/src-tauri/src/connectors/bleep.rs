//! bleep's own built-in tools.
use super::{Connector, ConnectorStatus, Risk, ToolSpec};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct BleepConnector;

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

#[async_trait]
impl Connector for BleepConnector {
    fn id(&self) -> &'static str {
        "bleep"
    }

    fn tools(&self) -> Vec<ToolSpec> {
        vec![ToolSpec {
            name: "bleep_now".into(),
            description: "The current local date, time, weekday and time zone on this Mac. Use it whenever the answer depends on the time.".into(),
            schema: json!({"type": "object", "properties": {}}),
            risk: Risk::Read,
        }]
    }

    fn status(&self) -> ConnectorStatus {
        ConnectorStatus::Connected { account: "this Mac".into() }
    }

    async fn call(&self, tool: &str, _args: Value) -> Result<Value, String> {
        match tool {
            "bleep_now" => Ok(bleep_now()),
            _ => Err(format!("Unknown tool: {tool}")),
        }
    }
}
