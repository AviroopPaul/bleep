//! Review check: does Claude Code wait for an MCP write call that sits on an approval card
//! for minutes? Holds one call for WAIT_SECS (default 370), then answers "declined".
//! Uses bleep's own Claude sign-in (~/.bleep/claude-home) and one small Haiku turn.
use async_trait::async_trait;
use bleep_lib::connectors::{Connector, ConnectorStatus, Risk, ToolSpec};
use bleep_lib::mcp::host::{Decision, Gate, ToolHost};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};

struct Probe;

#[async_trait]
impl Connector for Probe {
    fn id(&self) -> &'static str {
        "probe"
    }
    fn tools(&self) -> Vec<ToolSpec> {
        vec![ToolSpec { name: "probe_change", description: "Makes a test change. Needs the user's approval.", schema: json!({"type": "object", "properties": {"note": {"type": "string"}}}), risk: Risk::Write }]
    }
    fn status(&self) -> ConnectorStatus {
        ConnectorStatus::Connected { account: "probe".into() }
    }
    async fn call(&self, _t: &str, _a: Value) -> Result<Value, String> {
        Err("write only".into())
    }
    async fn execute(&self, _t: &str, _p: Value) -> Result<Value, String> {
        Ok(json!({"done": true}))
    }
}

struct SlowGate(Duration);

#[async_trait]
impl Gate for SlowGate {
    async fn ask(&self, _tool: &str, _payload: Value, _turn: Option<i64>) -> anyhow::Result<(i64, Decision)> {
        eprintln!("probe: card shown, waiting {:?}", self.0);
        tokio::time::sleep(self.0).await;
        eprintln!("probe: answering declined");
        Ok((1, Decision::Declined))
    }
    fn finish(&self, _id: i64, _ok: bool, _r: &str) {}
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let secs: u64 = std::env::var("WAIT_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(370);
    let host = ToolHost::new(vec![Arc::new(Probe)], Arc::new(SlowGate(Duration::from_secs(secs))), Default::default());
    let srv = bleep_lib::mcp::McpServer::start(bleep_lib::mcp::new_token(), host).await?;
    let dir = tempfile::tempdir()?;
    let cfg = dir.path().join("mcp.json");
    // The same config the app writes, so this checks what ships.
    let endpoint = bleep_lib::runtime::McpEndpoint { url: srv.url(), token: srv.token().into() };
    std::fs::write(&cfg, bleep_lib::runtime::claude::args::mcp_config_json(&endpoint).to_string())?;
    let bin = bleep_lib::runtime::claude::find_claude().ok_or_else(|| anyhow::anyhow!("claude not found"))?;
    let home = dirs::home_dir().unwrap().join(".bleep/claude-home");
    let mut child = tokio::process::Command::new(bin)
        .args(["-p", "Call the probe_change tool once with note hi. Then reply with exactly what the tool returned.", "--output-format", "stream-json", "--verbose", "--model", "haiku", "--tools", "", "--setting-sources", "", "--strict-mcp-config", "--mcp-config"])
        .arg(&cfg)
        .args(["--allowedTools", "mcp__bleep"])
        .env("CLAUDE_CONFIG_DIR", home)
        .env("MCP_TOOL_TIMEOUT", bleep_lib::runtime::claude::args::MCP_TOOL_TIMEOUT_MS)
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .current_dir(dir.path())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    let t0 = Instant::now();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    while let Some(l) = lines.next_line().await? {
        let v: Value = serde_json::from_str(&l).unwrap_or(Value::Null);
        match v["type"].as_str() {
            Some("user") => println!("[{:>4}s] tool_result: {}", t0.elapsed().as_secs(), v["message"]["content"][0]["content"]),
            Some("result") => println!("[{:>4}s] result: is_error={} text={}", t0.elapsed().as_secs(), v["is_error"], v["result"]),
            _ => {}
        }
    }
    child.wait().await?;
    Ok(())
}
