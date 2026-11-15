//! Remote MCP servers I add by URL (Phase 7). Each one becomes a bleep connector, so its
//! tools go through bleep's own MCP server and the approval gate.
pub mod names;
pub mod secrets;
pub mod tools;
pub mod auth;
pub mod client;
#[cfg(test)]
pub mod mock;

pub use client::RemoteConnector;
