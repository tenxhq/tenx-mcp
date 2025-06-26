use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use tenx_mcp::{schema::*, testutils::*, Error, Result, ServerAPI, ServerConn, ServerCtx};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
struct LifecycleTestServer {
    connect_count: Arc<AtomicU32>,
    disconnect_count: Arc<AtomicU32>,
    connection_ids: Arc<Mutex<Vec<String>>>,
}

impl Default for LifecycleTestServer {
    fn default() -> Self {
        Self {
            connect_count: Arc::new(AtomicU32::new(0)),
            disconnect_count: Arc::new(AtomicU32::new(0)),
            connection_ids: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl ServerConn for LifecycleTestServer {
    async fn on_connect(&self, _ctx: &ServerCtx) -> Result<()> {
        self.connect_count.fetch_add(1, Ordering::SeqCst);
        let mut ids = self.connection_ids.lock().await;
        // Store a unique identifier for this connection
        ids.push(format!(
            "conn_{}",
            self.connect_count.load(Ordering::SeqCst)
        ));
        Ok(())
    }

    async fn on_disconnect(&self) -> Result<()> {
        self.disconnect_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn initialize(
        &self,
        _context: &ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new("lifecycle_test_server").with_version("1.0.0"))
    }

    async fn list_tools(
        &self,
        _context: &ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListToolsResult> {
        Ok(ListToolsResult {
            tools: vec![Tool::new(
                "test_tool",
                ToolInputSchema::from_json_schema::<serde_json::Value>(),
            )
            .with_description("Test tool for lifecycle testing")],
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        _context: &ServerCtx,
        name: String,
        _arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<CallToolResult> {
        match name.as_str() {
            "test_tool" => Ok(CallToolResult::new()
                .with_text_content("Tool called")
                .is_error(false)),
            _ => Err(Error::ToolExecutionFailed {
                tool: name,
                message: "Tool not found".to_string(),
            }),
        }
    }
}

#[tokio::test]
async fn test_single_connection_lifecycle() {
    // First, create the server instance
    let server_impl = LifecycleTestServer::default();

    // Verify counts are zero before any server is started
    assert_eq!(
        server_impl.connect_count.load(Ordering::SeqCst),
        0,
        "Expected 0 on_connect calls before server starts"
    );
    assert_eq!(
        server_impl.disconnect_count.load(Ordering::SeqCst),
        0,
        "Expected 0 on_disconnect calls before server starts"
    );

    // Create server with our implementation
    let server_clone = server_impl.clone();
    let server = tenx_mcp::Server::default().with_connection(move || server_clone.clone());

    // Start server and get transport streams
    let (server_reader, server_writer, client_reader, client_writer) = make_duplex_pair();
    let server_handle = tenx_mcp::ServerHandle::from_stream(server, server_reader, server_writer)
        .await
        .unwrap();

    // Server is running but no client connected yet - counts should still be zero
    assert_eq!(
        server_impl.connect_count.load(Ordering::SeqCst),
        0,
        "Expected 0 on_connect calls after server start but before client connection"
    );
    assert_eq!(
        server_impl.disconnect_count.load(Ordering::SeqCst),
        0,
        "Expected 0 on_disconnect calls after server start but before client connection"
    );

    // Now connect a client
    let mut client = tenx_mcp::Client::new("test-client", "1.0.0");
    client
        .connect_stream(client_reader, client_writer)
        .await
        .unwrap();

    // Client needs to send initialize to actually establish the connection
    client.init().await.unwrap();

    // After client connects, on_connect should have been called exactly once
    assert_eq!(
        server_impl.connect_count.load(Ordering::SeqCst),
        1,
        "Expected exactly 1 on_connect call after client connection"
    );
    assert_eq!(
        server_impl.disconnect_count.load(Ordering::SeqCst),
        0,
        "Expected 0 on_disconnect calls while connected"
    );

    // Make a tool call to ensure the connection is active
    let result = client
        .call_tool("test_tool", serde_json::json!({}))
        .await
        .unwrap();

    // Verify we got the expected response
    assert_eq!(result.content.len(), 1);
    if let Content::Text(text_content) = &result.content[0] {
        assert_eq!(text_content.text, "Tool called");
    } else {
        panic!("Expected text content");
    }

    // After tool call, counts should remain the same
    assert_eq!(
        server_impl.connect_count.load(Ordering::SeqCst),
        1,
        "Connect count should remain 1 after tool call"
    );
    assert_eq!(
        server_impl.disconnect_count.load(Ordering::SeqCst),
        0,
        "Disconnect count should remain 0 while connected"
    );

    // Disconnect the client
    drop(client);

    // Stop the server gracefully
    server_handle.stop().await.unwrap();

    // Give some time for disconnect to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // After disconnect, on_disconnect should have been called exactly once
    assert_eq!(
        server_impl.connect_count.load(Ordering::SeqCst),
        1,
        "Connect count should remain 1 after disconnect"
    );
    assert_eq!(
        server_impl.disconnect_count.load(Ordering::SeqCst),
        1,
        "Expected exactly 1 on_disconnect call after shutdown"
    );
}

#[tokio::test]
async fn test_multiple_connections_to_same_server() {
    // Create a single server instance that will handle multiple connections
    let server_impl = LifecycleTestServer::default();

    // Verify counts are zero before server starts
    assert_eq!(
        server_impl.connect_count.load(Ordering::SeqCst),
        0,
        "Expected 0 on_connect calls before server starts"
    );

    // For TCP-like scenarios where one server handles multiple connections,
    // we need to simulate accepting multiple connections
    let server_clone = server_impl.clone();

    // First connection
    let (mut client1, handle1) =
        connected_client_and_server(move || Box::new(server_clone.clone()) as Box<dyn ServerConn>)
            .await
            .unwrap();

    // Initialize the connection
    client1.init().await.unwrap();

    // After first client connects
    assert_eq!(
        server_impl.connect_count.load(Ordering::SeqCst),
        1,
        "Expected 1 on_connect call after first client"
    );
    assert_eq!(
        server_impl.disconnect_count.load(Ordering::SeqCst),
        0,
        "Expected 0 on_disconnect calls with one client"
    );

    // Connect second client to the SAME server instance
    let server_clone2 = server_impl.clone();
    let (mut client2, handle2) =
        connected_client_and_server(move || Box::new(server_clone2.clone()) as Box<dyn ServerConn>)
            .await
            .unwrap();

    // Initialize the second connection
    client2.init().await.unwrap();

    // After second client connects
    assert_eq!(
        server_impl.connect_count.load(Ordering::SeqCst),
        2,
        "Expected 2 on_connect calls after second client"
    );
    assert_eq!(
        server_impl.disconnect_count.load(Ordering::SeqCst),
        0,
        "Expected 0 on_disconnect calls with two clients"
    );

    // Verify both connections work
    let result1 = client1
        .call_tool("test_tool", serde_json::json!({}))
        .await
        .unwrap();
    let result2 = client2
        .call_tool("test_tool", serde_json::json!({}))
        .await
        .unwrap();

    assert_eq!(result1.content.len(), 1);
    assert_eq!(result2.content.len(), 1);

    // Disconnect first client
    drop(client1);
    handle1.stop().await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    assert_eq!(
        server_impl.connect_count.load(Ordering::SeqCst),
        2,
        "Connect count should remain 2"
    );
    assert_eq!(
        server_impl.disconnect_count.load(Ordering::SeqCst),
        1,
        "Expected 1 on_disconnect after first client disconnects"
    );

    // Disconnect second client
    drop(client2);
    handle2.stop().await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    assert_eq!(
        server_impl.connect_count.load(Ordering::SeqCst),
        2,
        "Connect count should remain 2"
    );
    assert_eq!(
        server_impl.disconnect_count.load(Ordering::SeqCst),
        2,
        "Expected 2 on_disconnect calls after both clients disconnect"
    );
}

#[tokio::test]
async fn test_server_lifecycle_with_manual_connections() {
    // This test manually creates connections to ensure we test each step
    let server_impl = LifecycleTestServer::default();

    // Step 1: Server exists but not started
    assert_eq!(server_impl.connect_count.load(Ordering::SeqCst), 0);
    assert_eq!(server_impl.disconnect_count.load(Ordering::SeqCst), 0);

    // Step 2: Start server
    let server_clone = server_impl.clone();
    let server = tenx_mcp::Server::default().with_connection(move || server_clone.clone());

    let (server_reader, server_writer, client_reader1, client_writer1) = make_duplex_pair();
    let server_handle = tenx_mcp::ServerHandle::from_stream(server, server_reader, server_writer)
        .await
        .unwrap();

    // Server running, no clients yet
    assert_eq!(server_impl.connect_count.load(Ordering::SeqCst), 0);
    assert_eq!(server_impl.disconnect_count.load(Ordering::SeqCst), 0);

    // Step 3: Connect first client
    let mut client1 = tenx_mcp::Client::new("client-1", "1.0.0");
    client1
        .connect_stream(client_reader1, client_writer1)
        .await
        .unwrap();
    client1.init().await.unwrap();

    assert_eq!(server_impl.connect_count.load(Ordering::SeqCst), 1);
    assert_eq!(server_impl.disconnect_count.load(Ordering::SeqCst), 0);

    // Step 4: For second connection, we need a new server instance (simulating TCP accept)
    let server_clone2 = server_impl.clone();
    let server2 = tenx_mcp::Server::default().with_connection(move || server_clone2.clone());

    let (server_reader2, server_writer2, client_reader2, client_writer2) = make_duplex_pair();
    let server_handle2 =
        tenx_mcp::ServerHandle::from_stream(server2, server_reader2, server_writer2)
            .await
            .unwrap();

    let mut client2 = tenx_mcp::Client::new("client-2", "1.0.0");
    client2
        .connect_stream(client_reader2, client_writer2)
        .await
        .unwrap();
    client2.init().await.unwrap();

    assert_eq!(server_impl.connect_count.load(Ordering::SeqCst), 2);
    assert_eq!(server_impl.disconnect_count.load(Ordering::SeqCst), 0);

    // Step 5: Disconnect clients one by one
    drop(client1);
    server_handle.stop().await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    assert_eq!(server_impl.connect_count.load(Ordering::SeqCst), 2);
    assert_eq!(server_impl.disconnect_count.load(Ordering::SeqCst), 1);

    drop(client2);
    server_handle2.stop().await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    assert_eq!(server_impl.connect_count.load(Ordering::SeqCst), 2);
    assert_eq!(server_impl.disconnect_count.load(Ordering::SeqCst), 2);
}
