use async_trait::async_trait;
use tenx_mcp::schema::*;
use tenx_mcp::{Result, Server, ServerConn, ServerCtx};

/// Minimal test server
struct MinimalServerConn;

#[async_trait]
impl ServerConn for MinimalServerConn {
    async fn initialize(
        &self,
        _context: &ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        println!("SERVER: initialize() called!");
        Ok(InitializeResult::new("minimal-server", "1.0.0"))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("Server listening on: http://127.0.0.1:8888");
    println!("\nTest with:");
    println!("curl -X POST http://127.0.0.1:8888 -H 'Content-Type: application/json' -H 'MCP-Protocol-Version: 2025-06-18' -d '{{\"jsonrpc\":\"2.0\",\"id\":\"test-1\",\"method\":\"initialize\",\"params\":{{}}}}'");

    let handle = Server::default()
        .with_connection(|| MinimalServerConn)
        .serve_http("127.0.0.1:8888")
        .await?;

    // Wait for Ctrl+C signal
    tokio::signal::ctrl_c().await?;
    println!("\nShutting down server...");

    // Gracefully stop the server
    handle.stop().await?;

    Ok(())
}
