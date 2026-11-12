//! Starts bleep's MCP server and prints `<url> <token>`; runs until killed.
//! Used to record tests/fixtures/claude/tool_turn.jsonl against a real `claude`.
use bleep_lib::connectors::bleep::BleepConnector;
use bleep_lib::mcp::host::{NoGate, ToolHost};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let host = ToolHost::new(vec![Arc::new(BleepConnector)], Arc::new(NoGate), Default::default());
    let srv = bleep_lib::mcp::McpServer::start(bleep_lib::mcp::new_token(), host).await?;
    println!("{} {}", srv.url(), srv.token());
    tokio::signal::ctrl_c().await?;
    Ok(())
}
