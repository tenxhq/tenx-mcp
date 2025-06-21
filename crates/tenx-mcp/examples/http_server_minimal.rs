use async_trait::async_trait;
use tenx_mcp::schema::*;
use tenx_mcp::{HttpServerTransport, Result, Server, ServerConn, ServerCtx, ServerHandle};

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

    let mut server_transport = HttpServerTransport::new("127.0.0.1:8888");
    server_transport.start().await?;

    println!("Server listening on: http://127.0.0.1:8888");
    println!("\nTest with:");
    println!("curl -X POST http://127.0.0.1:8888 -H 'Content-Type: application/json' -H 'MCP-Protocol-Version: 2025-06-18' -d '{{\"jsonrpc\":\"2.0\",\"id\":\"test-1\",\"method\":\"initialize\",\"params\":{{}}}}'");

    let server = Server::default().with_connection(|| MinimalServerConn);
    let server_handle = ServerHandle::from_transport(server, Box::new(server_transport)).await?;

    // Keep running
    tokio::signal::ctrl_c().await?;
    server_handle.stop().await?;

    Ok(())
}
