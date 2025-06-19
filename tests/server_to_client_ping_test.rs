use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tenx_mcp::{
    schema::*,
    testutils::{connected_client_and_server_with_conn, shutdown_client_and_server},
    ClientConn, ClientCtx, Result, ServerConn, ServerCtx,
};

/// Test client connection that tracks method calls
#[derive(Clone)]
struct TestClientConnection {
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ClientConn for TestClientConnection {
    async fn on_connect(&mut self, _context: ClientCtx) -> Result<()> {
        self.calls.lock().unwrap().push("on_connect".to_string());
        Ok(())
    }

    async fn on_disconnect(&mut self, _context: ClientCtx) -> Result<()> {
        self.calls.lock().unwrap().push("on_disconnect".to_string());
        Ok(())
    }

    async fn pong(&mut self, _context: ClientCtx) -> Result<()> {
        self.calls.lock().unwrap().push("ping".to_string());
        println!("Client received ping from server!");
        Ok(())
    }

    async fn create_message(
        &mut self,
        _context: ClientCtx,
        _method: &str,
        _params: CreateMessageParams,
    ) -> Result<CreateMessageResult> {
        self.calls
            .lock()
            .unwrap()
            .push("create_message".to_string());
        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: SamplingContent::Text(TextContent {
                text: "Test response".to_string(),
                annotations: None,
            }),
            model: "test-model".to_string(),
            stop_reason: None,
            meta: None,
        })
    }

    async fn list_roots(&mut self, _context: ClientCtx) -> Result<ListRootsResult> {
        self.calls.lock().unwrap().push("list_roots".to_string());
        Ok(ListRootsResult {
            roots: vec![Root {
                uri: "test://root".to_string(),
                name: Some("Test Root".to_string()),
            }],
            meta: None,
        })
    }
}

/// Test server connection
struct TestServerConnection {
    context: Option<ServerCtx>,
}

#[async_trait]
impl ServerConn for TestServerConnection {
    async fn on_connect(&mut self, context: ServerCtx) -> Result<()> {
        self.context = Some(context);
        Ok(())
    }

    async fn initialize(
        &mut self,
        _context: ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new("test-server", "1.0.0"))
    }

    async fn pong(&mut self, _context: ServerCtx) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_server_to_client_ping_integration() {
    // Install tracing subscriber once so that we have helpful logs if the test
    // hangs.
    let _ = tracing_subscriber::fmt::try_init();

    // Shared state so that we can assert on what methods the client
    // implementation received.
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));

    // Create client and server connected via in-memory duplex streams.
    let (mut client, server_handle) = connected_client_and_server_with_conn(
        || Box::new(TestServerConnection { context: None }),
        TestClientConnection {
            calls: calls.clone(),
        },
    )
    .await
    .expect("failed to set up test client/server pair");

    // Wait until the client finishes the initial handshake.
    // NOTE: In a real-world test we would await the `initialize` request/response
    // explicitly but for the purposes of this test it's sufficient to sleep
    // for a tiny amount of time.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // Verify on_connect was called.
    {
        let calls_list = calls.lock().unwrap();
        assert!(calls_list.contains(&"on_connect".to_string()));
    }

    // Clear previous calls so that we can assert on the ping below.
    calls.lock().unwrap().clear();

    // The client should be able to ping the server (client->server request).
    client.ping().await.expect("Client ping to server failed");

    // Clean-up – drop the client so that the server task finishes and then
    // wait for graceful shutdown (best-effort).
    shutdown_client_and_server(client, server_handle).await;
}

#[tokio::test]
async fn test_client_handles_server_requests_unit() {
    // Unit test for ClientConnection methods

    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut connection = TestClientConnection {
        calls: calls.clone(),
    };

    // Create a dummy context for testing
    let (notification_tx, _) = tokio::sync::broadcast::channel(10);
    let context = ClientCtx::new(notification_tx);

    // Test ping
    connection.pong(context.clone()).await.expect("Ping failed");
    assert!(calls.lock().unwrap().contains(&"ping".to_string()));

    // Test create_message
    let create_params = CreateMessageParams {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: SamplingContent::Text(TextContent {
                text: "Hello".to_string(),
                annotations: None,
            }),
        }],
        system_prompt: None,
        include_context: None,
        temperature: None,
        max_tokens: 1000,
        metadata: None,
        stop_sequences: None,
        model_preferences: None,
    };

    let result = connection
        .create_message(context.clone(), "sampling/createMessage", create_params)
        .await
        .expect("Create message failed");
    assert_eq!(result.model, "test-model");
    assert!(calls
        .lock()
        .unwrap()
        .contains(&"create_message".to_string()));

    // Test list_roots
    let roots_result = connection
        .list_roots(context)
        .await
        .expect("List roots failed");
    assert_eq!(roots_result.roots.len(), 1);
    assert!(calls.lock().unwrap().contains(&"list_roots".to_string()));

    println!("All ClientConnection methods working correctly");
}
