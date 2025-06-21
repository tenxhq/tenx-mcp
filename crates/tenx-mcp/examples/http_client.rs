use std::collections::HashMap;
use tenx_mcp::{Client, Result, ServerAPI};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("Connecting to HTTP server at http://127.0.0.1:9999");

    // Create client
    let mut client = Client::new("example-http-client", "1.0.0");

    // Connect via HTTP
    let init_result = client.connect_http("http://127.0.0.1:9999").await?;

    println!(
        "Connected to server: {} v{}",
        init_result.server_info.name, init_result.server_info.version
    );

    // Test ping
    println!("\nTesting ping...");
    client.ping().await?;
    println!("Ping successful!");

    // List available tools
    println!("\nListing available tools...");
    let tools = client.list_tools(None).await?;
    for tool in &tools.tools {
        println!(
            "- Tool: {} - {}",
            tool.name,
            tool.description.as_deref().unwrap_or("No description")
        );
    }

    // Call the echo tool
    println!("\nCalling echo tool...");
    let mut args = HashMap::new();
    args.insert(
        "message".to_string(),
        serde_json::json!("Hello from HTTP client!"),
    );

    let result = client.call_tool("echo", Some(args)).await?;
    for content in &result.content {
        if let tenx_mcp::schema::Content::Text(text) = content {
            println!("Echo response: {}", text.text);
        }
    }

    println!("\nClient shutting down...");

    Ok(())
}
