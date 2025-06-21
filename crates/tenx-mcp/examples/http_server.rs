use async_trait::async_trait;
use std::collections::HashMap;
use tenx_mcp::{
    schema::*, Error, HttpServerTransport, Result, Server, ServerConn, ServerCtx, ServerHandle,
};

/// Example server connection handler
struct ExampleServerConn;

#[async_trait]
impl ServerConn for ExampleServerConn {
    async fn initialize(
        &self,
        _context: &ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        println!("Server: Client connected!");
        Ok(InitializeResult::new("example-http-server", "1.0.0"))
    }

    async fn pong(&self, _context: &ServerCtx) -> Result<()> {
        println!("Server: Received ping, sending pong!");
        Ok(())
    }

    async fn list_tools(
        &self,
        _context: &ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListToolsResult> {
        let schema = ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "message".to_string(),
                    serde_json::json!({
                        "type": "string",
                        "description": "The message to echo"
                    }),
                );
                props
            }),
            required: Some(vec!["message".to_string()]),
        };

        Ok(ListToolsResult::new()
            .with_tool(Tool::new("echo", schema).with_description("Echoes the input message")))
    }

    async fn call_tool(
        &self,
        _context: &ServerCtx,
        name: String,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<CallToolResult> {
        println!("Server: Tool '{}' called", name);

        if name == "echo" {
            let args = arguments.unwrap_or_default();
            let message = args
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("No message provided");

            println!("Server: Echoing message: {message}");

            Ok(CallToolResult::new().with_text_content(format!("Echo: {message}")))
        } else {
            Err(Error::ToolNotFound(name))
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("Starting HTTP server on http://127.0.0.1:0 (auto-assigned port)");

    // Start HTTP server
    let mut server_transport = HttpServerTransport::new("127.0.0.1:0");
    server_transport.start().await?;

    println!(
        "Server actually listening on: http://{}",
        server_transport.actual_bind_addr()
    );

    // Create and serve the MCP server
    let server = Server::default().with_connection(|| ExampleServerConn);

    let server_handle = ServerHandle::from_transport(server, Box::new(server_transport)).await?;

    println!("Server started! Waiting for connections...");
    println!("You can test with: cargo run --example http_client");

    // Wait for the server to stop
    server_handle.stop().await?;

    Ok(())
}
