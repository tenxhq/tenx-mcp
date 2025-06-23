//! HTTP MCP server example that pairs with http_client.rs
//!
//! This server listens on HTTP port 8080 (by default) and provides
//! a simple echo tool that can be called by clients.
//!
//! Usage:
//!   cargo run --example http_server [host] [port]  # HTTP mode
//!   cargo run --example http_server                # HTTP mode, defaults to 127.0.0.1:8080

use std::env;

use serde::{Deserialize, Serialize};
use tenx_mcp::{macros::*, schema::*, schemars, Result, Server, ServerCtx};
use tracing::info;


/// Echo tool input parameters
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct EchoParams {
    /// The message to echo back
    message: String,
}

/// Basic server connection that provides an echo tool
#[derive(Debug, Default)]
struct HttpServer {}

#[mcp_server]
/// HTTP MCP server that provides an echo tool
impl HttpServer {
    #[tool]
    /// Echoes back the provided message
    async fn echo(
        &self,
        _context: &ServerCtx,
        params: EchoParams,
    ) -> Result<CallToolResult> {
        Ok(CallToolResult::new()
            .with_text_content(params.message)
            .is_error(false))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    let (host, port) = if args.len() == 3 {
        (
            args[1].clone(),
            args[2].parse::<u16>().expect("Invalid port number"),
        )
    } else if args.len() == 1 {
        // Default to localhost:8080 to match http_client
        ("127.0.0.1".to_string(), 8080)
    } else {
        eprintln!("Usage: {} [host] [port]", args[0]);
        eprintln!("Example: {} 127.0.0.1 8080", args[0]);
        eprintln!("If no arguments provided, defaults to 127.0.0.1:8080");
        std::process::exit(1);
    };

    let addr = format!("{host}:{port}");

    // Create and run the server using HTTP
    info!("Starting HTTP MCP server on {}", addr);

    let handle = Server::default()
        .with_connection(HttpServer::default)
        .serve_http(addr)
        .await?;

    // Wait for Ctrl+C signal
    tokio::signal::ctrl_c().await?;
    info!("Shutting down HTTP server");

    // Gracefully stop the server
    handle.stop().await?;

    Ok(())
}
