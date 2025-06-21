use async_trait::async_trait;
use std::time::Duration;
use tenx_mcp::schema::*;
use tenx_mcp::{Client, HttpServerTransport, Result, Server, ServerConn, ServerCtx, ServerHandle};
use tokio::time::sleep;

/// Simple test server connection
struct TestServerConn;

#[async_trait]
impl ServerConn for TestServerConn {
    async fn initialize(
        &self,
        _context: &ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        println!("SERVER: initialize() called");
        Ok(InitializeResult::new("test-http-server", "1.0.0"))
    }

    async fn pong(&self, _context: &ServerCtx) -> Result<()> {
        println!("SERVER: pong() called");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Enable debug logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    // Start server on auto-assigned port
    let mut server_transport = HttpServerTransport::new("127.0.0.1:0");
    server_transport.start().await?;
    let bind_addr = format!("http://{}", server_transport.actual_bind_addr());

    println!("Server listening on: {bind_addr}");

    let server = Server::default().with_connection(|| TestServerConn);
    let server_handle = ServerHandle::from_transport(server, Box::new(server_transport)).await?;

    // Give server time to start
    sleep(Duration::from_millis(100)).await;

    // Create client
    println!("\nCreating client...");
    let mut client = Client::new("test-client", "1.0.0");

    // Connect and initialize
    println!("Connecting to {bind_addr} and sending initialize...");
    let init_result = client.connect_http(&bind_addr).await?;

    println!("Initialize response: {init_result:?}");

    // Cleanup
    drop(client);
    server_handle.stop().await?;

    Ok(())
}
