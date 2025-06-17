//! Basic MCP server example that pairs with basic_client.rs
//!
//! This server listens on TCP port 3000 (by default) and provides
//! a simple echo tool that can be called by the basic_client example.
//!
//! Usage:
//!   cargo run --example basic_server [host] [port]
//!   cargo run --example basic_server  # defaults to 127.0.0.1:3000

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::env;
use tenx_mcp::{connection::Connection, error::Error, schema::*, schemars, Result, Server};
use tracing::info;

const NAME: &str = "basic-server";
const VERSION: &str = "0.1.0";

/// Echo tool input parameters
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct EchoParams {
    /// The message to echo back
    message: String,
}

/// Basic server connection that provides an echo tool
#[derive(Debug, Default)]
struct BasicConnection {}

#[async_trait]
impl Connection for BasicConnection {
    async fn initialize(
        &mut self,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new(NAME, VERSION)
            .with_capabilities(ServerCapabilities::default().with_tools(None)))
    }

    async fn tools_list(&mut self) -> Result<ListToolsResult> {
        Ok(ListToolsResult::default().with_tool(
            Tool::new("echo", ToolInputSchema::from_json_schema::<EchoParams>())
                .with_description("Echoes back the provided message"),
        ))
    }

    async fn tools_call(
        &mut self,
        name: String,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        if name != "echo" {
            return Err(Error::ToolNotFound(name));
        }
        let params = match arguments {
            Some(args) => serde_json::from_value::<EchoParams>(args)?,
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
        // Default to localhost:3000 to match basic_client
        ("127.0.0.1".to_string(), 3000)
    } else {
        eprintln!("Usage: {} [host] [port]", args[0]);
        eprintln!("Example: {} 127.0.0.1 3000", args[0]);
        eprintln!("If no arguments provided, defaults to 127.0.0.1:3000");
        std::process::exit(1);
    };

    let addr = format!("{host}:{port}");

    // Create and run the server using the new simplified API
    info!("Starting basic MCP server on {}", addr);

    Server::default()
        .with_connection_factory(|| Box::new(BasicConnection::default()))
        .serve_tcp(addr)
        .await?;

    Ok(())
}
