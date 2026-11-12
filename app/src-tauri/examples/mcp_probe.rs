//! Starts bleep's MCP server and prints `<url> <token>`; runs until killed.
//! Used to record tests/fixtures/claude/tool_turn.jsonl against a real `claude`.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let srv = bleep_lib::mcp::McpServer::start(bleep_lib::mcp::new_token()).await?;
    println!("{} {}", srv.url(), srv.token());
    tokio::signal::ctrl_c().await?;
    Ok(())
}
