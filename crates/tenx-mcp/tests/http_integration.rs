/// Simplified HTTP integration tests with proper server lifecycle management
///
/// These tests demonstrate the current limitation where the HTTP server
/// needs explicit cleanup since ServerHandle doesn't automatically stop
/// the underlying HTTP server.
use async_trait::async_trait;
use std::collections::HashMap;
use tenx_mcp::{
    schema::*, Client, Error, HttpServerTransport, Result, Server, ServerAPI, ServerConn,
    ServerCtx, ServerHandle,
};

/// Minimal test server
#[derive(Clone)]
struct MinimalServer;

#[async_trait]
impl ServerConn for MinimalServer {
    async fn initialize(
        &self,
        _context: &ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new("minimal-server", "1.0.0"))
    }

    async fn pong(&self, _context: &ServerCtx) -> Result<()> {
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
                    "text".to_string(),
                    serde_json::json!({
                        "type": "string",
                        "description": "Text to echo"
                    }),
                );
                props
            }),
            required: None,
        };

        Ok(ListToolsResult::new().with_tool(Tool::new("echo", schema)))
    }

    async fn call_tool(
        &self,
        _context: &ServerCtx,
        name: String,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<CallToolResult> {
        if name == "echo" {
            let text = arguments
                .as_ref()
                .and_then(|args| args.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("empty");
            Ok(CallToolResult::new().with_text_content(format!("Echo: {text}")))
        } else {
            Err(Error::ToolNotFound(name))
        }
    }
}

#[tokio::test]
async fn test_http_basic_communication() {
    let _ = tracing_subscriber::fmt::try_init();

    // Start HTTP server
    let mut http_transport = HttpServerTransport::new("127.0.0.1:0");
    http_transport
        .start()
        .await
        .expect("Failed to start HTTP server");
    let bind_addr = http_transport.actual_bind_addr().to_string();

    // Create MCP server
    let mcp_server = Server::default().with_connection(|| MinimalServer);
    let server_handle = ServerHandle::from_transport(mcp_server, Box::new(http_transport))
        .await
        .expect("Failed to create server handle");

    // Create and connect client
    let mut client = Client::new("test-client", "1.0.0");
    let init_result = client
        .connect_http(&format!("http://{bind_addr}"))
        .await
        .expect("Failed to connect client");

    assert_eq!(init_result.server_info.name, "minimal-server");

    // Test ping
    client.ping().await.expect("Ping failed");

    // Test tool call
    let mut args = HashMap::new();
    args.insert("text".to_string(), serde_json::json!("Hello HTTP"));

    let result = client
        .call_tool("echo", Some(args))
        .await
        .expect("Tool call failed");

    if let Some(Content::Text(text)) = result.content.first() {
        assert_eq!(text.text, "Echo: Hello HTTP");
    } else {
        panic!("Expected text content");
    }

    // Clean up - this is the current limitation
    // The HTTP server will continue running after this test completes
    // In a real application, you would need to manage the HTTP server lifecycle separately
    drop(client);
    drop(server_handle);

    // Note: Without proper shutdown handling in HttpServerTransport,
    // the server continues running in the background
}

/// Test that demonstrates multiple clients can connect
#[tokio::test]
async fn test_http_multiple_clients() {
    let _ = tracing_subscriber::fmt::try_init();

    // Start server
    let mut http_transport = HttpServerTransport::new("127.0.0.1:0");
    http_transport
        .start()
        .await
        .expect("Failed to start HTTP server");
    let bind_addr = http_transport.actual_bind_addr().to_string();

    let mcp_server = Server::default().with_connection(|| MinimalServer);
    let _server_handle = ServerHandle::from_transport(mcp_server, Box::new(http_transport))
        .await
        .expect("Failed to create server handle");

    // Connect multiple clients
    let mut client1 = Client::new("client1", "1.0.0");
    client1
        .connect_http(&format!("http://{bind_addr}"))
        .await
        .expect("Client 1 failed to connect");

    let mut client2 = Client::new("client2", "1.0.0");
    client2
        .connect_http(&format!("http://{bind_addr}"))
        .await
        .expect("Client 2 failed to connect");

    // Both should be able to ping
    client1.ping().await.expect("Client 1 ping failed");
    client2.ping().await.expect("Client 2 ping failed");

    // Both should be able to call tools
    let tools1 = client1
        .list_tools(None)
        .await
        .expect("Client 1 list_tools failed");
    let tools2 = client2
        .list_tools(None)
        .await
        .expect("Client 2 list_tools failed");

    assert_eq!(tools1.tools.len(), 1);
    assert_eq!(tools2.tools.len(), 1);
    assert_eq!(tools1.tools[0].name, "echo");
    assert_eq!(tools2.tools[0].name, "echo");
}

/// Test error handling
#[tokio::test]
async fn test_http_error_handling() {
    let _ = tracing_subscriber::fmt::try_init();

    // Start server
    let mut http_transport = HttpServerTransport::new("127.0.0.1:0");
    http_transport
        .start()
        .await
        .expect("Failed to start HTTP server");
    let bind_addr = http_transport.actual_bind_addr().to_string();

    let mcp_server = Server::default().with_connection(|| MinimalServer);
    let _server_handle = ServerHandle::from_transport(mcp_server, Box::new(http_transport))
        .await
        .expect("Failed to create server handle");

    // Connect client
    let mut client = Client::new("test-client", "1.0.0");
    client
        .connect_http(&format!("http://{bind_addr}"))
        .await
        .expect("Failed to connect");

    // Test non-existent tool
    let result = client.call_tool("non_existent", None).await;
    assert!(result.is_err());
    match result.err().unwrap() {
        Error::MethodNotFound(msg) => assert!(msg.contains("non_existent")),
        e => panic!("Expected MethodNotFound, got: {e:?}"),
    }
}

