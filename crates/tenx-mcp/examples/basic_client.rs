use serde::{Deserialize, Serialize};
use std::env;
use tenx_mcp::{schema, schemars, Client, Result, ServerAPI};
use tracing::info;

/// Echo tool input parameters - must match the server definition
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct EchoParams {
    /// The message to echo back
    message: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let addr = if args.len() == 3 {
        format!("{}:{}", args[1], args[2])
    } else {
        "localhost:3000".to_string()
    };

    info!("Connecting to MCP server at {}", addr);

    // Create client and connect using the new convenience method
    let mut client = Client::new("example-client", "0.1.0");
    let server_info = client.connect_tcp(&addr).await?;
    info!("Connected to server: {}", server_info.server_info.name);

    // List available tools
    let tools = client.list_tools(None).await?;
    info!("\nAvailable tools:");
    for tool in &tools.tools {
        info!(
            "  - {}: {}",
            tool.name,
            tool.description.as_deref().unwrap_or("")
        );
    }

    // Call the echo tool
    info!("\nCalling echo tool...");
    let params = EchoParams {
        message: "Hello from tenx-mcp client!".to_string(),
    };
    // If "echo" took no arguments, you would pass `()` like so:
    // let result = client.call_tool("echo_no_args", ()).await?;

    let result = client.call_tool("echo", params).await?;

    // Assume text response
    if let Some(schema::Content::Text(text_content)) = result.content.first() {
        info!("Response: {}", text_content.text);
    }

    Ok(())
}
