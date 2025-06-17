use serde::{Deserialize, Serialize};
use std::env;
use tenx_mcp::{
    schemars, Client, ClientCapabilities, Content, Implementation, Result, TcpClientTransport,
};
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

    // Create client and connect
    let mut client = Client::new();
    let transport = Box::new(TcpClientTransport::new(addr));
    client.connect(transport).await?;

    // Initialize the connection
    let client_info = Implementation {
        name: "example-client".to_string(),
        version: "0.1.0".to_string(),
    };

    let capabilities = ClientCapabilities::default();

    let init_result = client.initialize(client_info, capabilities).await?;
    info!("Connected to server: {}", init_result.server_info.name);

    // List available tools
    let tools = client.list_tools().await?;
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
    let echo_params = EchoParams {
        message: "Hello from tenx-mcp client!".to_string(),
    };
    let result = client
        .call_tool("echo", Some(serde_json::to_value(&echo_params)?))
        .await?;

    // Assume text response
    if let Some(Content::Text(text_content)) = result.content.first() {
        info!("Response: {}", text_content.text);
    }

    Ok(())
}
