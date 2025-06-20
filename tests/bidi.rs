/// Test bi-dirctional client-server and server-client messages
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tenx_mcp::{
    schema::*,
    testutils::{connected_client_and_server_with_conn, shutdown_client_and_server},
    ClientAPI, ClientConn, ClientCtx, Result, ServerAPI, ServerConn, ServerCtx,
};

/// Simple test client that responds to server requests
#[derive(Clone)]
struct SimpleClient {
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ClientConn for SimpleClient {
    async fn pong(&self, _context: ClientCtx) -> Result<()> {
        self.calls.lock().unwrap().push("client_pong".to_string());
        Ok(())
    }

    async fn list_roots(&self, _context: ClientCtx) -> Result<ListRootsResult> {
        self.calls.lock().unwrap().push("list_roots".to_string());
        Ok(ListRootsResult {
            roots: vec![Root {
                uri: "file:///test".to_string(),
                name: Some("Test Root".to_string()),
            }],
            meta: None,
        })
    }
}

/// Simple test server that makes requests to the client
struct SimpleServer {
    stored_context: Arc<Mutex<Option<ServerCtx>>>,
}

#[async_trait]
impl ServerConn for SimpleServer {
    async fn on_connect(&self, context: ServerCtx) -> Result<()> {
        // Store the context for later use
        *self.stored_context.lock().unwrap() = Some(context);
        Ok(())
    }

    async fn initialize(
        &self,
        _context: ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new("simple-server", "1.0.0"))
    }

    /// A simple tool that demonstrates server->client requests
    async fn tools_call(
        &self,
        mut context: ServerCtx,
        name: String,
        _arguments: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<CallToolResult> {
        match name.as_str() {
            "test_ping" => {
                // Server pings the client
                context.ping().await?;
                Ok(CallToolResult::new().with_text_content("Ping successful"))
            }
            "test_roots" => {
                // Server asks client for roots
                let roots = context.list_roots().await?;
                let text = format!("Client has {} roots", roots.roots.len());
                Ok(CallToolResult::new().with_text_content(text))
            }
            _ => Err(tenx_mcp::Error::ToolExecutionFailed {
                tool: name,
                message: "Unknown tool".to_string(),
            }),
        }
    }

    async fn tools_list(
        &self,
        _context: ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListToolsResult> {
        Ok(ListToolsResult::new()
            .with_tool(
                Tool::new("test_ping", ToolInputSchema::default())
                    .with_description("Test server->client ping"),
            )
            .with_tool(
                Tool::new("test_roots", ToolInputSchema::default())
                    .with_description("Test server->client list_roots"),
            ))
    }
}

#[tokio::test]
async fn test_server_to_client_requests() {
    let _ = tracing_subscriber::fmt::try_init();

    let client_calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let server_context = Arc::new(Mutex::new(None));

    // Create client and server
    let (mut client, server_handle) = connected_client_and_server_with_conn(
        {
            let ctx = server_context.clone();
            move || {
                Box::new(SimpleServer {
                    stored_context: ctx.clone(),
                })
            }
        },
        SimpleClient {
            calls: client_calls.clone(),
        },
    )
    .await
    .expect("Failed to create client/server pair");

    // Initialize
    client
        .initialize(
            LATEST_PROTOCOL_VERSION.to_string(),
            ClientCapabilities::default(),
            Implementation {
                name: "test-client".to_string(),
                version: "1.0.0".to_string(),
            },
        )
        .await
        .expect("Initialize failed");

    // Small delay to ensure initialization completes
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Test 1: Server pings client
    println!("\n=== Test: Server pings client ===");
    client_calls.lock().unwrap().clear();

    let result = client
        .call_tool("test_ping", None)
        .await
        .expect("Tool call failed");
    println!("Result: {result:?}");

    // Verify client received ping
    assert!(client_calls
        .lock()
        .unwrap()
        .contains(&"client_pong".to_string()));

    // Test 2: Server gets roots from client
    println!("\n=== Test: Server gets roots from client ===");
    client_calls.lock().unwrap().clear();

    let result = client
        .call_tool("test_roots", None)
        .await
        .expect("Tool call failed");
    println!("Result: {result:?}");

    // Verify client received list_roots request
    assert!(client_calls
        .lock()
        .unwrap()
        .contains(&"list_roots".to_string()));

    // Clean up
    shutdown_client_and_server(client, server_handle).await;
}
