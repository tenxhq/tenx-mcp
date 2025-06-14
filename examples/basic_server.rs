use std::env;

use async_trait::async_trait;
use serde_json::Value;
use tenx_mcp::{MCPServer, Result, ToolHandler};
use tokio::{net::TcpListener, signal};
use tracing::{error, info};

/// Example echo tool handler
struct EchoToolHandler;

#[async_trait]
impl ToolHandler for EchoToolHandler {
    fn metadata(&self) -> tenx_mcp::schema::Tool {
        tenx_mcp::schema::Tool {
            name: "echo".to_string(),
            description: Some("Echoes back the provided message".to_string()),
            input_schema: tenx_mcp::schema::ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some({
                    let mut props = std::collections::HashMap::new();
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
        }
    }

    async fn execute(&self, arguments: Option<Value>) -> Result<Vec<tenx_mcp::schema::Content>> {
        let message = arguments
            .as_ref()
            .and_then(|v| v.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("No message provided");

        Ok(vec![tenx_mcp::schema::Content::Text(
            tenx_mcp::schema::TextContent {
                text: message.to_string(),
                annotations: None,
            },
        )])
    }
}

/// Example add tool handler
struct AddToolHandler;

#[async_trait]
impl ToolHandler for AddToolHandler {
    fn metadata(&self) -> tenx_mcp::schema::Tool {
        tenx_mcp::schema::Tool {
            name: "add".to_string(),
            description: Some("Adds two numbers together".to_string()),
            input_schema: tenx_mcp::schema::ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some({
                    let mut props = std::collections::HashMap::new();
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
        }
    }

    async fn execute(&self, arguments: Option<Value>) -> Result<Vec<tenx_mcp::schema::Content>> {
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

        Ok(vec![tenx_mcp::schema::Content::Text(
            tenx_mcp::schema::TextContent {
                text: format!("{a} + {b} = {result}"),
                annotations: None,
            },
        )])
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
                        let mut server = MCPServer::new(
                            "tenx-mcp-example-server".to_string(),
                            "0.1.0".to_string(),
                        );

                        server.register_tool(Box::new(EchoToolHandler)).await;
                        server.register_tool(Box::new(AddToolHandler)).await;

                        // Create transport from the accepted connection
                        let transport = Box::new(tenx_mcp::transport::TcpServerTransport::new(stream));

                        // Handle the connection in a separate task
                        tokio::spawn(async move {
                            info!("Handling connection from {}", peer_addr);
                            match server.serve(transport).await {
                                Ok(()) => info!("Connection from {} closed", peer_addr),
                                Err(e) => error!("Error handling connection from {}: {}", peer_addr, e),
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
