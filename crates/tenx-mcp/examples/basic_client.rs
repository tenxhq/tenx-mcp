use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use tenx_mcp::{schema, schemars, Client, Result, ServerAPI};
use tracing::info;

/// Echo tool input parameters - must match the server definition
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct EchoParams {
    /// The message to echo back
    message: String,
}

#[derive(Parser)]
#[command(name = "basic_client")]
#[command(about = "Basic MCP client that connects to echo server", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Connect using TCP
    Tcp {
        /// Host to connect to
        #[arg(long, default_value = "localhost")]
        host: String,
        /// Port to connect to
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
    },
    /// Connect using HTTP
    Http {
        /// URL to connect to (e.g., http://localhost:8080)
        #[arg(short, long, default_value = "http://localhost:8080")]
        url: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    // Create client and connect using the appropriate method
    let mut client = Client::new("example-client", "0.1.0");

    let (mode, server_info) = match cli.command.unwrap_or(Commands::Tcp {
        host: "localhost".to_string(),
        port: 3000,
    }) {
        Commands::Tcp { host, port } => {
            let addr = format!("{host}:{port}");
            info!("Connecting to MCP server at {} (TCP)", addr);
            let server_info = client.connect_tcp(&addr).await?;
            ("TCP", server_info)
        }
        Commands::Http { url } => {
            let url = if url.starts_with("http://") || url.starts_with("https://") {
                url
            } else {
                format!("http://{}", url)
            };
            info!("Connecting to MCP server at {} (HTTP)", url);
            let server_info = client.connect_http(&url).await?;
            ("HTTP", server_info)
        }
    };

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
        message: format!("Hello from tenx-mcp {} client!", mode),
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
