//! Basic MCP server example that pairs with basic_client.rs
//!
//! This server can run in three modes:
//! 1. TCP mode: listens on TCP port 3000 (by default)
//! 2. HTTP mode: listens on HTTP port 8080 (by default)
//! 3. Stdio mode: communicates via stdin/stdout
//!
//! All modes provide a simple echo tool that can be called by clients.
//!
//! Usage:
//!   cargo run --example basic_server tcp [host] [port]  # TCP mode
//!   cargo run --example basic_server http [host] [port] # HTTP mode
//!   cargo run --example basic_server stdio              # Stdio mode
//!   cargo run --example basic_server                    # Default to TCP mode on 127.0.0.1:3000

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use tenx_mcp::{mcp_server, schema::*, schemars, tool, Result, Server, ServerCtx};
use tracing::info;

/// Echo tool input parameters
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct EchoParams {
    /// The message to echo back
    message: String,
}

/// Basic server connection that provides an echo tool
#[derive(Debug, Default)]
struct BasicServer {}

#[mcp_server]
/// Basic MCP server that provides an echo tool
impl BasicServer {
    #[tool]
    /// Echoes back the provided message
    async fn echo(&self, _context: &ServerCtx, params: EchoParams) -> Result<CallToolResult> {
        Ok(CallToolResult::new()
            .with_text_content(params.message)
            .is_error(false))
    }
}

#[derive(Parser)]
#[command(name = "basic_server")]
#[command(about = "Basic MCP server with echo tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run server in TCP mode
    Tcp {
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind to
        #[arg(short, long, default_value_t = 3000)]
        port: u16,
    },
    /// Run server in HTTP mode
    Http {
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind to
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
    /// Run server in stdio mode
    Stdio,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Tcp {
        host: "127.0.0.1".to_string(),
        port: 3000,
    }) {
        Commands::Stdio => {
            // Run in stdio mode - no logging to avoid interfering with JSON-RPC
            Server::default()
                .with_connection(BasicServer::default)
                .serve_stdio()
                .await?;
        }
        Commands::Tcp { host, port } => {
            // Initialize logging for network modes
            tracing_subscriber::fmt::init();

            let addr = format!("{host}:{port}");
            info!("Starting TCP MCP server on {}", addr);

            Server::default()
                .with_connection(BasicServer::default)
                .serve_tcp(addr)
                .await?;
        }
        Commands::Http { host, port } => {
            // Initialize logging for network modes
            tracing_subscriber::fmt::init();

            let addr = format!("{host}:{port}");
            info!("Starting HTTP MCP server on {}", addr);

            let handle = Server::default()
                .with_connection(BasicServer::default)
                .serve_http(addr)
                .await?;

            // Wait for Ctrl+C signal
            tokio::signal::ctrl_c().await?;
            info!("Shutting down HTTP server");

            // Gracefully stop the server
            handle.stop().await?;
        }
    }

    Ok(())
}
