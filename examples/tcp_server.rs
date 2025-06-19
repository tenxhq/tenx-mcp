//! Example TCP server demonstrating the simplified server API
//!
//! This example shows how to create an MCP server that accepts TCP connections
//! and provides echo and add tools using the Connection trait with minimal boilerplate.

use async_trait::async_trait;
use std::collections::HashMap;
use std::env;
use tenx_mcp::{error::Error, schema::*, server_connection::ServerConnection, Result, Server};

/// TCP server connection that provides echo and add tools
struct TcpExampleConnection {
    server_info: Implementation,
    capabilities: ServerCapabilities,
}

impl TcpExampleConnection {
    fn new(server_info: Implementation, capabilities: ServerCapabilities) -> Self {
        Self {
            server_info,
            capabilities,
        }
    }
}

#[async_trait]
impl ServerConnection for TcpExampleConnection {
    async fn initialize(
        &mut self,
        _context: tenx_mcp::server_connection::ServerConnectionContext,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(
            InitializeResult::new(&self.server_info.name, &self.server_info.version)
                .with_capabilities(self.capabilities.clone()),
        )
    }

    async fn tools_list(
        &mut self,
        _context: tenx_mcp::server_connection::ServerConnectionContext,
    ) -> Result<ListToolsResult> {
        let echo_schema = ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "message".to_string(),
                    serde_json::json!({
                        "type": "string",
                        "description": "The message to echo back"
                    }),
                );
                props
            }),
            required: Some(vec!["message".to_string()]),
        };

        let add_schema = ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "a".to_string(),
                    serde_json::json!({
                        "type": "number",
                        "description": "First number"
                    }),
                );
                props.insert(
                    "b".to_string(),
                    serde_json::json!({
                        "type": "number",
                        "description": "Second number"
                    }),
                );
                props
            }),
            required: Some(vec!["a".to_string(), "b".to_string()]),
        };

        Ok(ListToolsResult::new()
            .with_tool(
                Tool::new("echo", echo_schema).with_description("Echoes back the provided message"),
            )
            .with_tool(Tool::new("add", add_schema).with_description("Adds two numbers together")))
    }

    async fn tools_call(
        &mut self,
        _context: tenx_mcp::server_connection::ServerConnectionContext,
        name: String,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        match name.as_str() {
            "echo" => {
                let message = arguments
                    .as_ref()
                    .and_then(|v| v.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("No message provided");

                Ok(CallToolResult::new()
                    .with_text_content(message.to_string())
                    .is_error(false))
            }
            "add" => {
                let a = arguments
                    .as_ref()
                    .and_then(|v| v.get("a"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);

                let b = arguments
                    .as_ref()
                    .and_then(|v| v.get("b"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);

                let result = a + b;

                Ok(CallToolResult::new()
                    .with_text_content(format!("{a} + {b} = {result}"))
                    .is_error(false))
            }
            _ => Err(Error::ToolExecutionFailed {
                tool: name,
                message: "Tool not found".to_string(),
            }),
        }
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
        // Default to localhost:3000
        ("127.0.0.1".to_string(), 3000)
    } else {
        eprintln!("Usage: {} [host] [port]", args[0]);
        eprintln!("Example: {} 127.0.0.1 3000", args[0]);
        eprintln!("If no arguments provided, defaults to 127.0.0.1:3000");
        std::process::exit(1);
    };

    let addr = format!("{host}:{port}");

    // Create server configuration
    let server_info = Implementation {
        name: "tenx-mcp-tcp-example".to_string(),
        version: "0.1.0".to_string(),
    };

    let capabilities = ServerCapabilities::default();

    // Use the new simplified API to serve TCP connections
    Server::default()
        .with_connection_factory(move || {
            Box::new(TcpExampleConnection::new(
                server_info.clone(),
                capabilities.clone(),
            ))
        })
        .serve_tcp(addr)
        .await?;

    Ok(())
}
