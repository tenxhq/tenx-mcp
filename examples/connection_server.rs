use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::env;
use tenx_mcp::{
    schema::{
        CallToolResult, Content, InitializeResult, ListToolsResult, TextContent, Tool,
        ToolInputSchema,
    },
    Connection, ConnectionContext, MCPServer, MCPServerHandle, Result,
};
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info};

/// Example connection implementation
struct MyConnection {
    // Connection-specific state can be stored here
    request_count: u64,
    context: Option<ConnectionContext>,
}

impl MyConnection {
    fn new() -> Self {
        Self {
            request_count: 0,
            context: None,
        }
    }
}

#[async_trait]
impl Connection for MyConnection {
    async fn on_connect(&mut self, context: ConnectionContext) -> Result<()> {
        info!("New connection established");
        self.context = Some(context);
        Ok(())
    }

    async fn on_disconnect(&mut self) -> Result<()> {
        info!("Connection closed after {} requests", self.request_count);
        Ok(())
    }

    async fn initialize(
        &mut self,
        protocol_version: String,
        _capabilities: tenx_mcp::schema::ClientCapabilities,
        client_info: tenx_mcp::schema::Implementation,
    ) -> Result<InitializeResult> {
        info!(
            "Initializing connection from {} {} (protocol: {})",
            client_info.name, client_info.version, protocol_version
        );

        self.request_count += 1;

        Ok(InitializeResult::new("connection-example-server", "0.1.0").with_tools(true))
    }

    async fn ping(&mut self) -> Result<()> {
        self.request_count += 1;
        info!("Ping received (request #{})", self.request_count);
        Ok(())
    }

    async fn tools_list(&mut self) -> Result<ListToolsResult> {
        self.request_count += 1;

        let tools = vec![
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
            Tool {
                name: "request_count".to_string(),
                description: Some(
                    "Returns the number of requests made in this connection".to_string(),
                ),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some(HashMap::new()),
                    required: None,
                },
                annotations: None,
            },
        ];

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn tools_call(
        &mut self,
        name: String,
        arguments: Option<Value>,
    ) -> Result<CallToolResult> {
        self.request_count += 1;

        let content = match name.as_str() {
            "echo" => {
                let message = arguments
                    .as_ref()
                    .and_then(|v| v.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("No message provided");

                vec![Content::Text(TextContent {
                    text: message.to_string(),
                    annotations: None,
                })]
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

                vec![Content::Text(TextContent {
                    text: format!("{a} + {b} = {result}"),
                    annotations: None,
                })]
            }
            "request_count" => {
                vec![Content::Text(TextContent {
                    text: format!(
                        "This connection has handled {} requests",
                        self.request_count
                    ),
                    annotations: None,
                })]
            }
            _ => {
                return Err(tenx_mcp::MCPError::ToolExecutionFailed {
                    tool: name,
                    message: "Tool not found".to_string(),
                });
            }
        };

        // Send a notification after tool execution
        if name == "add" {
            if let Some(ctx) = &self.context {
                ctx.send_notification(tenx_mcp::schema::ServerNotification::LoggingMessage {
                    level: tenx_mcp::schema::LoggingLevel::Info,
                    logger: Some("math".to_string()),
                    data: serde_json::json!({
                        "operation": "addition",
                        "result": content[0],
                    }),
                })?;
            }
        }

        let mut result = CallToolResult::new().is_error(false);
        for c in content {
            result = result.with_content(c);
        }
        Ok(result)
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

                        // Create a new server with connection factory
                        let server = MCPServer::default()
                        .with_connection_factory(|| Box::new(MyConnection::new()));

                        // Create transport from the accepted connection
                        let transport = Box::new(tenx_mcp::transport::TcpServerTransport::new(stream));

                        // Handle the connection in a separate task
                        tokio::spawn(async move {
                            info!("Handling connection from {}", peer_addr);
                            match MCPServerHandle::new(server, transport).await {
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
