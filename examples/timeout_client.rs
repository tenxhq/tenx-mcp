//! Example MCP client that demonstrates timeout and retry behavior
//!
//! This client connects to the timeout_server and tests various scenarios:
//! - Retrying operations that fail intermittently
//! - Handling timeouts for slow operations
//! - Dealing with non-retryable errors

use std::env;
use std::time::Duration;
use tenx_mcp::{
    client::{ClientConfig, MCPClient},
    error::{Error, Result},
    retry::RetryConfig,
    schema::*,
    transport::TcpTransport,
};
use tracing::{error, info};

async fn test_reliable_operation(client: &mut MCPClient) -> Result<()> {
    info!("\n=== Testing Reliable Operation ===");
    info!("This should succeed immediately...");

    match client
        .call_tool("reliable_operation".to_string(), None)
        .await
    {
        Ok(result) => {
            info!("✓ Success: {:?}", result.content);
        }
        Err(e) => {
            error!("✗ Unexpected failure: {}", e);
        }
    }

    Ok(())
}

async fn test_flakey_operation(client: &mut MCPClient) -> Result<()> {
    info!("\n=== Testing Flakey Operation ===");
    info!("This operation fails 2 times before succeeding.");
    info!("With retry enabled, it should eventually succeed...");

    match client.call_tool("flakey_operation".to_string(), None).await {
        Ok(result) => {
            info!("✓ Success after retries: {:?}", result.content);
        }
        Err(e) => {
            error!("✗ Failed even with retries: {}", e);
        }
    }

    Ok(())
}

async fn test_slow_operation(client: &mut MCPClient) -> Result<()> {
    info!("\n=== Testing Slow Operation ===");
    info!("This operation takes 5 seconds, but our timeout is 2 seconds.");
    info!("It should timeout and retry, but still fail...");

    match client.call_tool("slow_operation".to_string(), None).await {
        Ok(_) => {
            error!("✗ Unexpected success - should have timed out");
        }
        Err(e) => {
            info!("✓ Expected timeout: {}", e);
            // Verify it's actually a timeout error
            if !matches!(e, Error::Timeout { .. }) {
                error!("Error was not a timeout: {:?}", e);
            }
        }
    }

    Ok(())
}

async fn test_broken_operation(client: &mut MCPClient) -> Result<()> {
    info!("\n=== Testing Broken Operation ===");
    info!("This operation always fails with a non-retryable error.");
    info!("Should fail immediately without retries...");

    match client.call_tool("broken_operation".to_string(), None).await {
        Ok(_) => {
            error!("✗ Unexpected success");
        }
        Err(e) => {
            info!("✓ Expected non-retryable error: {}", e);
            // Verify it's actually non-retryable
            if e.is_retryable() {
                error!("Error was retryable when it shouldn't be: {:?}", e);
            }
        }
    }

    Ok(())
}

async fn test_custom_retry_config(host: &str, port: u16) -> Result<()> {
    info!("\n=== Testing Custom Retry Configuration ===");
    info!("Creating a client with very short timeouts and few retries...");

    // Create a new client with custom configuration
    let config = ClientConfig {
        retry: RetryConfig {
            max_attempts: 2,
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_millis(100),
            backoff_multiplier: 2.0,
            timeout: Duration::from_millis(500), // Very short timeout
        },
        request_timeout: Duration::from_secs(1),
    };

    let mut client = MCPClient::with_config(config);
    let transport = TcpTransport::new(format!("{host}:{port}"));

    client.connect(Box::new(transport)).await?;
    client
        .initialize(
            Implementation {
                name: "timeout-test-client-custom".to_string(),
                version: "1.0.0".to_string(),
            },
            ClientCapabilities::default(),
        )
        .await?;

    // This should timeout very quickly
    match client.call_tool("slow_operation".to_string(), None).await {
        Ok(_) => {
            error!("✗ Unexpected success");
        }
        Err(e) => {
            info!("✓ Quick timeout as expected: {}", e);
        }
    }

    Ok(())
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
        // Default to localhost:3001 (matching timeout_server default)
        ("127.0.0.1".to_string(), 3001)
    } else {
        eprintln!("Usage: {} [host] [port]", args[0]);
        eprintln!("Example: {} 127.0.0.1 3001", args[0]);
        eprintln!("If no arguments provided, defaults to 127.0.0.1:3001");
        std::process::exit(1);
    };

    info!("Starting Timeout Test Client");
    info!("Connecting to {}:{}", host, port);

    // Create client with default retry configuration
    let retry_config = RetryConfig {
        max_attempts: 3,
        initial_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(2),
        backoff_multiplier: 2.0,
        timeout: Duration::from_secs(2), // 2 second timeout
    };

    let config = ClientConfig {
        retry: retry_config,
        request_timeout: Duration::from_secs(5),
    };

    let mut client = MCPClient::with_config(config);

    // Create transport
    let transport = TcpTransport::new(format!("{host}:{port}"));

    // Connect and initialize
    info!("Connecting to server...");
    client.connect(Box::new(transport)).await?;

    let init_result = client
        .initialize(
            Implementation {
                name: "timeout-test-client".to_string(),
                version: "1.0.0".to_string(),
            },
            ClientCapabilities::default(),
        )
        .await?;

    info!(
        "Connected! Server: {} v{}",
        init_result.server_info.name, init_result.server_info.version
    );

    // List available tools
    info!("\nAvailable tools:");
    let tools = client.list_tools().await?;
    for tool in &tools.tools {
        info!(
            "  - {}: {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
    }

    // Run tests
    if let Err(e) = test_reliable_operation(&mut client).await {
        error!("Reliable operation test failed: {}", e);
    }

    if let Err(e) = test_flakey_operation(&mut client).await {
        error!("Flakey operation test failed: {}", e);
    }

    if let Err(e) = test_slow_operation(&mut client).await {
        error!("Slow operation test failed: {}", e);
    }

    if let Err(e) = test_broken_operation(&mut client).await {
        error!("Broken operation test failed: {}", e);
    }

    // Test with custom configuration
    info!("\n{}", "=".repeat(50));
    if let Err(e) = test_custom_retry_config(&host, port).await {
        error!("Custom retry config test failed: {}", e);
    }

    info!("\n=== All tests completed ===");
    info!("Summary:");
    info!("- Reliable operation: Should succeed immediately");
    info!("- Flakey operation: Should fail 2 times then succeed");
    info!("- Slow operation: Should timeout after 2 seconds");
    info!("- Broken operation: Should fail with non-retryable error");
    info!("- Custom retry config: Demonstrates configuration options");

    Ok(())
}
