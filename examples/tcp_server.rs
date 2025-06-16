//! Example TCP server demonstrating the modern Connection trait pattern
//!
//! This example shows how to create an MCP server that accepts TCP connections
//! and provides echo and add tools using the Connection trait.

use async_trait::async_trait;
use std::collections::HashMap;
use std::env;
use tenx_mcp::{connection::Connection, error::MCPError, schema::*, MCPServer, Result};
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info};

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
impl Connection for TcpExampleConnection {
    async fn initialize(
        &mut self,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new(&self.server_info.name, &self.server_info.version)
            .with_capabilities(self.capabilities.clone()))
    }

    async fn tools_list(&mut self) -> Result<ListToolsResult> {
        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: "echo".to_string(),
                    description: Some("Echoes back the provided message".to_string()),
                    input_schema: ToolInputSchema {
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
                    },
                    annotations: None,
                },
                Tool {
                    name: "add".to_string(),
                    description: Some("Adds two numbers together".to_string()),
                    input_schema: ToolInputSchema {
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
                    },
                    annotations: None,
                },
            ],
            next_cursor: None,
        })
    }

    async fn tools_call(
        &mut self,
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

                Ok(CallToolResult {
                    content: vec![Content::Text(TextContent {
                        text: message.to_string(),
                        annotations: None,
                    })],
                    is_error: Some(false),
                    meta: None,
                })
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

                Ok(CallToolResult {
                    content: vec![Content::Text(TextContent {
                        text: format!("{a} + {b} = {result}"),
                        annotations: None,
                    })],
                    is_error: Some(false),
                    meta: None,
                })
            }
            _ => Err(MCPError::ToolExecutionFailed {
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

    // Create TCP listener
    let listener = TcpListener::bind(&addr).await?;
    info!("MCP server listening on {}", addr);

    // Accept connections in a loop
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer_addr)) => {
                        info!("New connection from {}", peer_addr);

                        // Create a new server instance for each connection
                        let server_info = Implementation {
                            name: "tenx-mcp-tcp-example".to_string(),
                            version: "0.1.0".to_string(),
                        };

                        let capabilities = ServerCapabilities::default();

                        let server = MCPServer::default()
                            .with_connection_factory(move || {
                                Box::new(TcpExampleConnection::new(server_info.clone(), capabilities.clone()))
                            });

                        // Create transport from the accepted connection
                        let transport = Box::new(tenx_mcp::transport::TcpServerTransport::new(stream));

                        // Handle the connection in a separate task
                        tokio::spawn(async move {
                            info!("Handling connection from {}", peer_addr);
                            match tenx_mcp::MCPServerHandle::new(server, transport).await {
                                Ok(server_handle) => {
                                    info!("Server handle created for {}", peer_addr);
                                    if let Err(e) = server_handle.handle.await {
                                        error!("Server task failed for {}: {}", peer_addr, e);
                                    } else {
                                        info!("Connection from {} closed", peer_addr);
                                    }
                                },
                                Err(e) => error!("Error creating server handle for {}: {}", peer_addr, e),
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept connection: {}", e);
                    }
                }
            }
            _ = signal::ctrl_c() => {
                info!("\nShutting down server...");
                break;
            }
        }
    }

    info!("Server stopped");
    Ok(())
}
