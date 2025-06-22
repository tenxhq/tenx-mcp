//! HTTP MCP server example that pairs with http_client.rs
//!
//! This server listens on HTTP port 8080 (by default) and provides
//! a simple echo tool that can be called by clients.
//!
//! Usage:
//!   cargo run --example http_server [host] [port]  # HTTP mode
//!   cargo run --example http_server                # HTTP mode, defaults to 127.0.0.1:8080

use std::{collections::HashMap, env};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tenx_mcp::{
    schema::*, schemars, Error, HttpServerTransport, Result, Server, ServerConn, ServerCtx,
    ServerHandle,
};
use tracing::info;

const NAME: &str = "http-server";
const VERSION: &str = "0.1.0";

/// Echo tool input parameters
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct EchoParams {
    /// The message to echo back
    message: String,
}

/// Basic server connection that provides an echo tool
#[derive(Debug, Default)]
struct HttpServer {}

#[async_trait]
impl ServerConn for HttpServer {
    async fn initialize(
        &self,
        _context: &ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new(NAME, VERSION)
            .with_capabilities(ServerCapabilities::default().with_tools(None)))
    }

    async fn list_tools(
        &self,
        _context: &ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListToolsResult> {
        Ok(ListToolsResult::default().with_tool(
            Tool::new("echo", ToolInputSchema::from_json_schema::<EchoParams>())
                .with_description("Echoes back the provided message"),
        ))
    }

    async fn call_tool(
        &self,
        _context: &ServerCtx,
        name: String,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<CallToolResult> {
        if name != "echo" {
            return Err(Error::ToolNotFound(name));
        }
        let params = match arguments {
            Some(args) => serde_json::from_value::<EchoParams>(serde_json::to_value(args)?)?,
            None => return Err(Error::InvalidParams("No arguments provided".to_string())),
        };
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

    // Start HTTP transport
    let mut http_transport = HttpServerTransport::new(&addr);
    http_transport.start().await?;

    // Create MCP server
    let mcp_server = Server::default().with_connection(HttpServer::default);
    let _server_handle = ServerHandle::from_transport(mcp_server, Box::new(http_transport)).await?;

    // Keep the server running
    tokio::signal::ctrl_c().await?;
    info!("Shutting down HTTP server");

    Ok(())
}

