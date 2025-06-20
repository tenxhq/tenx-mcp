//! Basic MCP server example that pairs with basic_client.rs
//!
//! This server can run in two modes:
//! 1. TCP mode: listens on TCP port 3000 (by default)
//! 2. Stdio mode: communicates via stdin/stdout
//!
//! Both modes provide a simple echo tool that can be called by clients.
//!
//! Usage:
//!   cargo run --example basic_server [host] [port]  # TCP mode
//!   cargo run --example basic_server                # TCP mode, defaults to 127.0.0.1:3000
//!   cargo run --example basic_server --stdio        # Stdio mode

use std::{collections::HashMap, env};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tenx_mcp::{schema::*, schemars, Error, Result, Server, ServerConn, ServerCtx};
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
impl ServerConn for BasicConnection {
    async fn initialize(
        &self,
        _context: ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new(NAME, VERSION)
            .with_capabilities(ServerCapabilities::default().with_tools(None)))
    }

    async fn tools_list(
        &self,
        _context: ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListToolsResult> {
        Ok(ListToolsResult::default().with_tool(
            Tool::new("echo", ToolInputSchema::from_json_schema::<EchoParams>())
                .with_description("Echoes back the provided message"),
        ))
    }

    async fn tools_call(
        &self,
        _context: ServerCtx,
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
    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    // Check for --stdio flag
    let is_stdio = args.iter().any(|arg| arg == "--stdio");

    // Only initialize logging for TCP mode
    // In stdio mode, logging would interfere with JSON-RPC communication
    if !is_stdio {
        tracing_subscriber::fmt::init();
    }

    if is_stdio {
        // Run in stdio mode - no logging to avoid interfering with JSON-RPC
        Server::default()
            .with_connection(BasicConnection::default)
            .serve_stdio()
            .await?;
    } else {
        // Run in TCP mode
        let (host, port) = if args.len() == 3 {
            (
                args[1].clone(),
                args[2].parse::<u16>().expect("Invalid port number"),
            )
        } else if args.len() == 1 {
            // Default to localhost:3000 to match basic_client
            ("127.0.0.1".to_string(), 3000)
        } else {
            eprintln!("Usage: {} [host] [port] or {} --stdio", args[0], args[0]);
            eprintln!("Example: {} 127.0.0.1 3000", args[0]);
            eprintln!("Example: {} --stdio", args[0]);
            eprintln!("If no arguments provided, defaults to 127.0.0.1:3000");
            std::process::exit(1);
        };

        let addr = format!("{host}:{port}");

        // Create and run the server using the new simplified API
        info!("Starting basic MCP server on {}", addr);

        Server::default()
            .with_connection(BasicConnection::default)
            .serve_tcp(addr)
            .await?;
    }

    Ok(())
}
