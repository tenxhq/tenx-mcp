//! Example demonstrating retry and timeout behavior in MCP
//!
//! This example shows how the client handles transient failures,
//! timeouts, and non-retryable errors with the retry mechanism.

#![allow(dead_code, unused_imports)]

use async_trait::async_trait;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use tenx_mcp::{
    error::{MCPError, Result},
    schema::*,
    server::ToolHandler,
};
use tokio::time::sleep;
use tracing::info;

/// A tool that fails a configurable number of times before succeeding
struct FlakeyTool {
    fail_count: AtomicU32,
    failures_before_success: u32,
}

impl FlakeyTool {
    fn new(failures_before_success: u32) -> Self {
        Self {
            fail_count: AtomicU32::new(0),
            failures_before_success,
        }
    }
}

#[async_trait]
impl ToolHandler for FlakeyTool {
    fn metadata(&self) -> Tool {
        Tool {
            name: "flakey_tool".to_string(),
            description: Some("A tool that fails intermittently".to_string()),
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
            info!("FlakeyTool failing (attempt {})", count + 1);
            // Return a retryable error
            Err(MCPError::ConnectionClosed)
        } else {
            info!("FlakeyTool succeeding (attempt {})", count + 1);
            Ok(vec![Content::Text(TextContent {
                text: format!("Success after {} attempts", count + 1),
                annotations: None,
            })])
        }
    }
}

/// A tool that takes too long to execute
struct SlowTool;

#[async_trait]
impl ToolHandler for SlowTool {
    fn metadata(&self) -> Tool {
        Tool {
            name: "slow_tool".to_string(),
            description: Some("A tool that takes too long".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
            },
            annotations: None,
        }
    }

    async fn execute(&self, _arguments: Option<serde_json::Value>) -> Result<Vec<Content>> {
        info!("SlowTool starting execution...");
        sleep(Duration::from_secs(5)).await;
        Ok(vec![Content::Text(TextContent {
            text: "This should timeout".to_string(),
            annotations: None,
        })])
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_target(false).init();

    // TODO: This example requires test transport which is only available in tests
    // For now, we'll just print a message
    info!("Retry/timeout example - requires test transport implementation");

    /*
    // Create test transports
    let (client_transport, server_transport) = TestTransport::create_pair();

    // Create and configure server
    let mut server = MCPServer::new("retry-example-server".to_string(), "0.1.0".to_string())
        .with_capabilities(ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: Some(true),
            }),
            ..Default::default()
        });

    // Register tools
    server.register_tool(Box::new(FlakeyTool::new(2))).await;
    server.register_tool(Box::new(SlowTool)).await;

    // Start server in background
    let server_handle = tokio::spawn(async move {
        if let Err(e) = server.serve(server_transport).await {
            eprintln!("Server error: {}", e);
        }
    });

    // Create client with custom retry configuration
    let mut client = MCPClient::with_config(ClientConfig {
        retry: RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
            backoff_multiplier: 2.0,
            timeout: Duration::from_secs(2),
        },
        request_timeout: Duration::from_secs(1),
    });

    // Connect and initialize
    client.connect(client_transport).await?;
    client
        .initialize(
            Implementation {
                name: "retry-test-client".to_string(),
                version: "0.1.0".to_string(),
            },
            ClientCapabilities::default(),
        )
        .await?;

    info!("Testing flakey tool (should succeed after retries)...");
    match client.call_tool("flakey_tool".to_string(), None).await {
        Ok(result) => {
            info!("FlakeyTool result: {:?}", result.content);
        }
        Err(e) => {
            info!("FlakeyTool failed: {}", e);
        }
    }

    info!("\nTesting slow tool (should timeout)...");
    match client.call_tool("slow_tool".to_string(), None).await {
        Ok(_) => {
            info!("SlowTool unexpectedly succeeded");
        }
        Err(e) => {
            info!("SlowTool failed as expected: {}", e);
            assert!(matches!(e, MCPError::Timeout { .. }));
        }
    }

    info!("\nTesting non-existent tool (should not retry)...");
    match client
        .call_tool("non_existent".to_string(), None)
        .await
    {
        Ok(_) => {
            info!("Non-existent tool unexpectedly succeeded");
        }
        Err(e) => {
            info!("Non-existent tool failed as expected: {}", e);
            assert!(!e.is_retryable());
        }
    }

    // Cleanup
    server_handle.abort();
    */

    Ok(())
}
