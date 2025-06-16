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
    error::{MCPError, Result},
    schema::*,
    server::{MCPServer, ToolHandler},
    transport::TcpServerTransport,
};
use tokio::net::TcpListener;
use tokio::signal;
use tokio::time::sleep;
use tracing::{error, info, warn};

/// A tool that fails a configurable number of times before succeeding
struct FlakeyTool {
    fail_count: Arc<AtomicU32>,
    failures_before_success: u32,
}

impl FlakeyTool {
    fn new(failures_before_success: u32) -> Self {
        Self {
            fail_count: Arc::new(AtomicU32::new(0)),
            failures_before_success,
        }
    }
}

#[async_trait]
impl ToolHandler for FlakeyTool {
    fn metadata(&self) -> Tool {
        Tool {
            name: "flakey_operation".to_string(),
            description: Some("Simulates a flaky network operation".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
            },
            annotations: None,
        }
    }

    async fn execute(&self, _arguments: Option<serde_json::Value>) -> Result<Vec<Content>> {
        let count = self.fail_count.fetch_add(1, Ordering::SeqCst);

        if count < self.failures_before_success {
            warn!("FlakeyTool failing (attempt {})", count + 1);
            // Simulate a transient network error
            Err(MCPError::ConnectionClosed)
        } else {
            info!("FlakeyTool succeeding (attempt {})", count + 1);
            // Reset counter for next invocation
            if count >= self.failures_before_success {
                self.fail_count.store(0, Ordering::SeqCst);
            }
            Ok(vec![Content::Text(TextContent {
                text: format!("Success after {} attempts", count + 1),
                annotations: None,
            })])
        }
    }
}

/// A tool that takes too long to execute
struct SlowTool {
    delay_seconds: u64,
}

impl SlowTool {
    fn new(delay_seconds: u64) -> Self {
        Self { delay_seconds }
    }
}

#[async_trait]
impl ToolHandler for SlowTool {
    fn metadata(&self) -> Tool {
        Tool {
            name: "slow_operation".to_string(),
            description: Some("Simulates a slow operation that may timeout".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
            },
            annotations: None,
        }
    }

    async fn execute(&self, _arguments: Option<serde_json::Value>) -> Result<Vec<Content>> {
        info!(
            "SlowTool starting execution (will take {} seconds)...",
            self.delay_seconds
        );
        sleep(Duration::from_secs(self.delay_seconds)).await;
        info!("SlowTool completed");

        Ok(vec![Content::Text(TextContent {
            text: "Operation completed successfully".to_string(),
            annotations: None,
        })])
    }
}

/// A tool that always fails with a non-retryable error
struct BrokenTool;

#[async_trait]
impl ToolHandler for BrokenTool {
    fn metadata(&self) -> Tool {
        Tool {
            name: "broken_operation".to_string(),
            description: Some("Always fails with a non-retryable error".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
            },
            annotations: None,
        }
    }

    async fn execute(&self, _arguments: Option<serde_json::Value>) -> Result<Vec<Content>> {
        // Return an error that is not retryable
        Err(MCPError::InvalidParams {
            method: "broken_operation".to_string(),
            message: "This operation is permanently broken".to_string(),
        })
    }
}

/// A tool that works reliably
struct ReliableTool;

#[async_trait]
impl ToolHandler for ReliableTool {
    fn metadata(&self) -> Tool {
        Tool {
            name: "reliable_operation".to_string(),
            description: Some("Always succeeds immediately".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
            },
            annotations: None,
        }
    }

    async fn execute(&self, _arguments: Option<serde_json::Value>) -> Result<Vec<Content>> {
        Ok(vec![Content::Text(TextContent {
            text: "Reliable operation completed".to_string(),
            annotations: None,
        })])
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
                        let mut server = MCPServer::new(
                            "timeout-test-server".to_string(),
                            "1.0.0".to_string(),
                        );

                        // Register tools with different behaviors
                        server.register_tool(Box::new(FlakeyTool::new(2))).await; // Fails first 2 attempts
                        server.register_tool(Box::new(SlowTool::new(5))).await; // Takes 5 seconds
                        server.register_tool(Box::new(BrokenTool)).await;
                        server.register_tool(Box::new(ReliableTool)).await;

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
