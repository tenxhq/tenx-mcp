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
use tenx_mcp::{schema, Error, Result, Server, ServerConn, ServerCtx};
use tokio::time::sleep;
use tracing::{info, warn};

/// Connection that demonstrates various timeout and retry scenarios
struct TimeoutTestConnection {
    server_info: schema::Implementation,
    capabilities: schema::ServerCapabilities,
    flakey_fail_count: Arc<AtomicU32>,
    failures_before_success: u32,
    slow_delay_seconds: u64,
}

impl TimeoutTestConnection {
    fn new(
        server_info: schema::Implementation,
        capabilities: schema::ServerCapabilities,
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
impl ServerConn for TimeoutTestConnection {
    async fn initialize(
        &self,
        _context: &ServerCtx,
        _protocol_version: String,
        _capabilities: schema::ClientCapabilities,
        _client_info: schema::Implementation,
    ) -> Result<schema::InitializeResult> {
        Ok(
            schema::InitializeResult::new(&self.server_info.name, &self.server_info.version)
                .with_capabilities(self.capabilities.clone()),
        )
    }

    async fn tools_list(
        &self,
        _context: &ServerCtx,
        _cursor: Option<schema::Cursor>,
    ) -> Result<schema::ListToolsResult> {
        let object_schema = schema::ToolInputSchema {
            schema_type: "object".to_string(),
            properties: None,
            required: None,
        };

        Ok(schema::ListToolsResult::new()
            .with_tool(
                schema::Tool::new("flakey_operation", object_schema.clone())
                    .with_description("Simulates a flaky network operation"),
            )
            .with_tool(
                schema::Tool::new("slow_operation", object_schema.clone())
                    .with_description("Simulates a slow operation that may timeout"),
            )
            .with_tool(
                schema::Tool::new("broken_operation", object_schema.clone())
                    .with_description("Always fails with a non-retryable error"),
            )
            .with_tool(
                schema::Tool::new("reliable_operation", object_schema)
                    .with_description("Always succeeds immediately"),
            ))
    }

    async fn tools_call(
        &self,
        _context: &ServerCtx,
        name: String,
        _arguments: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<schema::CallToolResult> {
        match name.as_str() {
            "flakey_operation" => {
                let count = self.flakey_fail_count.fetch_add(1, Ordering::SeqCst);

                if count < self.failures_before_success {
                    warn!("FlakeyTool failing (attempt {})", count + 1);
                    // Simulate a transient network error
                    Err(Error::ConnectionClosed)
                } else {
                    info!("FlakeyTool succeeding (attempt {})", count + 1);
                    // Reset counter for next invocation
                    if count >= self.failures_before_success {
                        self.flakey_fail_count.store(0, Ordering::SeqCst);
                    }
                    Ok(schema::CallToolResult::new()
                        .with_text_content(format!("Success after {} attempts", count + 1))
                        .is_error(false))
                }
            }
            "slow_operation" => {
                info!(
                    "SlowTool starting execution (will take {} seconds)...",
                    self.slow_delay_seconds
                );
                sleep(Duration::from_secs(self.slow_delay_seconds)).await;
                info!("SlowTool completed");

                Ok(schema::CallToolResult::new()
                    .with_text_content("Operation completed successfully")
                    .is_error(false))
            }
            "broken_operation" => {
                // Return an error that is not retryable
                Err(Error::InvalidParams(
                    "broken_operation: This operation is permanently broken".to_string(),
                ))
            }
            "reliable_operation" => Ok(schema::CallToolResult::new()
                .with_text_content("Reliable operation completed")
                .is_error(false)),
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

    // Create server configuration
    let server_info = schema::Implementation {
        name: "timeout-test-server".to_string(),
        version: "1.0.0".to_string(),
    };

    let capabilities = schema::ServerCapabilities::default();

    // Log server capabilities
    info!("Starting timeout test server on {}", addr);
    info!("Server capabilities:");
    info!("  - Simulates intermittent failures");
    info!("  - Demonstrates slow operations");
    info!("  - Shows non-retryable errors");
    info!("  - Provides a reliable operation");
    info!("");
    info!("Available tools:");
    info!("  - flakey_operation: Fails 2 times before succeeding");
    info!("  - slow_operation: Takes 5 seconds to complete");
    info!("  - broken_operation: Always fails with non-retryable error");
    info!("  - reliable_operation: Always succeeds immediately");

    // Use the new simplified API to serve TCP connections
    Server::default()
        .with_connection(move || {
            TimeoutTestConnection::new(
                server_info.clone(),
                capabilities.clone(),
                2, // Fails first 2 attempts
                5, // Takes 5 seconds
            )
        })
        .serve_tcp(addr)
        .await?;

    Ok(())
}
