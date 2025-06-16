//! Example of a flaky MCP server that demonstrates timeout and retry scenarios
//!
//! This server implements tools that exhibit various failure modes:
//! - Intermittent failures that succeed after retries
//! - Slow operations that cause timeouts
//! - Non-retryable errors

use async_trait::async_trait;
use std::env;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tenx_mcp::{
    connection::Connection,
    error::{MCPError, Result},
    schema::*,
    server::MCPServer,
    transport::TcpServerTransport,
};
use tokio::net::TcpListener;
use tokio::signal;
use tokio::time::sleep;
use tracing::{error, info, warn};

/// Connection that demonstrates various timeout and retry scenarios
struct TimeoutTestConnection {
    server_info: Implementation,
    capabilities: ServerCapabilities,
    flakey_fail_count: Arc<AtomicU32>,
    failures_before_success: u32,
    slow_delay_seconds: u64,
}

impl TimeoutTestConnection {
    fn new(
        server_info: Implementation,
        capabilities: ServerCapabilities,
        failures_before_success: u32,
        slow_delay_seconds: u64,
    ) -> Self {
        Self {
            server_info,
            capabilities,
            flakey_fail_count: Arc::new(AtomicU32::new(0)),
            failures_before_success,
            slow_delay_seconds,
        }
    }
}

#[async_trait]
impl Connection for TimeoutTestConnection {
    async fn initialize(
        &mut self,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(
            InitializeResult::new(&self.server_info.name, &self.server_info.version)
                .with_capabilities(self.capabilities.clone()),
        )
    }

    async fn tools_list(&mut self) -> Result<ListToolsResult> {
        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: "flakey_operation".to_string(),
                    description: Some("Simulates a flaky network operation".to_string()),
                    input_schema: ToolInputSchema {
                        schema_type: "object".to_string(),
                        properties: None,
                        required: None,
                    },
                    annotations: None,
                },
                Tool {
                    name: "slow_operation".to_string(),
                    description: Some("Simulates a slow operation that may timeout".to_string()),
                    input_schema: ToolInputSchema {
                        schema_type: "object".to_string(),
                        properties: None,
                        required: None,
                    },
                    annotations: None,
                },
                Tool {
                    name: "broken_operation".to_string(),
                    description: Some("Always fails with a non-retryable error".to_string()),
                    input_schema: ToolInputSchema {
                        schema_type: "object".to_string(),
                        properties: None,
                        required: None,
                    },
                    annotations: None,
                },
                Tool {
                    name: "reliable_operation".to_string(),
                    description: Some("Always succeeds immediately".to_string()),
                    input_schema: ToolInputSchema {
                        schema_type: "object".to_string(),
                        properties: None,
                        required: None,
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
        _arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        match name.as_str() {
            "flakey_operation" => {
                let count = self.flakey_fail_count.fetch_add(1, Ordering::SeqCst);

                if count < self.failures_before_success {
                    warn!("FlakeyTool failing (attempt {})", count + 1);
                    // Simulate a transient network error
                    Err(MCPError::ConnectionClosed)
                } else {
                    info!("FlakeyTool succeeding (attempt {})", count + 1);
                    // Reset counter for next invocation
                    if count >= self.failures_before_success {
                        self.flakey_fail_count.store(0, Ordering::SeqCst);
                    }
                    Ok(CallToolResult {
                        content: vec![Content::Text(TextContent {
                            text: format!("Success after {} attempts", count + 1),
                            annotations: None,
                        })],
                        is_error: Some(false),
                        meta: None,
                    })
                }
            }
            "slow_operation" => {
                info!(
                    "SlowTool starting execution (will take {} seconds)...",
                    self.slow_delay_seconds
                );
                sleep(Duration::from_secs(self.slow_delay_seconds)).await;
                info!("SlowTool completed");

                Ok(CallToolResult {
                    content: vec![Content::Text(TextContent {
                        text: "Operation completed successfully".to_string(),
                        annotations: None,
                    })],
                    is_error: Some(false),
                    meta: None,
                })
            }
            "broken_operation" => {
                // Return an error that is not retryable
                Err(MCPError::InvalidParams {
                    method: "broken_operation".to_string(),
                    message: "This operation is permanently broken".to_string(),
                })
            }
            "reliable_operation" => Ok(CallToolResult {
                content: vec![Content::Text(TextContent {
                    text: "Reliable operation completed".to_string(),
                    annotations: None,
                })],
                is_error: Some(false),
                meta: None,
            }),
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
    tracing_subscriber::fmt().with_target(false).init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();

    let (host, port) = if args.len() == 3 {
        (
            args[1].clone(),
            args[2].parse::<u16>().expect("Invalid port number"),
        )
    } else if args.len() == 1 {
        // Default to localhost:3001 (different from basic_server)
        ("127.0.0.1".to_string(), 3001)
    } else {
        eprintln!("Usage: {} [host] [port]", args[0]);
        eprintln!("Example: {} 127.0.0.1 3001", args[0]);
        eprintln!("If no arguments provided, defaults to 127.0.0.1:3001");
        std::process::exit(1);
    };

    let addr = format!("{host}:{port}");

    // Create TCP listener
    let listener = TcpListener::bind(&addr).await?;
    info!("Timeout Test Server listening on {}", addr);

    // Accept connections in a loop
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, peer_addr)) => {
                        info!("New connection from {}", peer_addr);

                        // Create a new server instance for each connection
                        let server_info = Implementation {
                            name: "timeout-test-server".to_string(),
                            version: "1.0.0".to_string(),
                        };

                        let capabilities = ServerCapabilities::default();

                        let server = MCPServer::default()
                            .with_connection_factory(move || {
                                Box::new(TimeoutTestConnection::new(
                                    server_info.clone(),
                                    capabilities.clone(),
                                    2, // Fails first 2 attempts
                                    5, // Takes 5 seconds
                                ))
                            });

                        info!("Registered tools for {}:", peer_addr);
                        info!("  - flakey_operation: Fails 2 times before succeeding");
                        info!("  - slow_operation: Takes 5 seconds to complete");
                        info!("  - broken_operation: Always fails with non-retryable error");
                        info!("  - reliable_operation: Always succeeds immediately");

                        // Create transport from the accepted connection
                        let transport = Box::new(TcpServerTransport::new(stream));

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
