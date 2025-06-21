/// Tests for bidirectional communication between MCP client and server
///
/// This test suite validates that:
/// 1. Clients can make requests to servers (normal flow)
/// 2. Servers can make requests to clients during request handling (reverse flow)
/// 3. Both directions support full request/response semantics
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tenx_mcp::{
    schema::*,
    testutils::{connected_client_and_server_with_conn, shutdown_client_and_server},
    ClientAPI, ClientConn, ClientCtx, Result, ServerAPI, ServerConn, ServerCtx,
};

/// Tracks method calls for test verification
type CallTracker = Arc<Mutex<Vec<String>>>;

/// Test client that can respond to server-initiated requests
#[derive(Clone)]
struct TestClient {
    calls: CallTracker,
}

impl TestClient {
    fn new() -> (Self, CallTracker) {
        let calls = CallTracker::default();
        (
            Self {
                calls: calls.clone(),
            },
            calls,
        )
    }

    fn track_call(&self, method: &str) {
        self.calls.lock().unwrap().push(method.to_string());
    }
}

#[async_trait]
impl ClientConn for TestClient {
    async fn pong(&self, _context: &ClientCtx) -> Result<()> {
        self.track_call("client_pong");
        Ok(())
    }

    async fn list_roots(&self, _context: &ClientCtx) -> Result<ListRootsResult> {
        self.track_call("list_roots");
        Ok(ListRootsResult {
            roots: vec![Root {
                uri: "file:///test".to_string(),
                name: Some("Test Root".to_string()),
            }],
            meta: None,
        })
    }

    async fn create_message(
        &self,
        _context: &ClientCtx,
        _method: &str,
        params: CreateMessageParams,
    ) -> Result<CreateMessageResult> {
        self.track_call("create_message");

        let request_text = match &params.messages.first() {
            Some(msg) => match &msg.content {
                SamplingContent::Text(t) => &t.text,
                _ => "non-text",
            },
            None => "no message",
        };

        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: SamplingContent::Text(TextContent {
                text: format!("Client received: {request_text}"),
                annotations: None,
            }),
            model: "test-model".to_string(),
            stop_reason: None,
            meta: None,
        })
    }
}

/// Test server that initiates requests to the client mid-request
#[derive(Clone)]
struct TestServer {
    calls: CallTracker,
}

impl TestServer {
    fn new() -> (Self, CallTracker) {
        let calls = CallTracker::default();
        (
            Self {
                calls: calls.clone(),
            },
            calls,
        )
    }

    fn track_call(&self, method: &str) {
        self.calls.lock().unwrap().push(method.to_string());
    }
}

#[async_trait]
impl ServerConn for TestServer {
    async fn pong(&self, _context: &ServerCtx) -> Result<()> {
        self.track_call("server_pong");
        Ok(())
    }

    async fn initialize(
        &self,
        _context: &ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new("test-server", "1.0.0"))
    }

    async fn tools_call(
        &self,
        context: &ServerCtx,
        name: String,
        _arguments: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<CallToolResult> {
        self.track_call(&format!("tool_{name}"));

        match name.as_str() {
            "ping_client" => {
                let mut ctx = context.clone();
                ctx.ping().await?;
                Ok(CallToolResult::new().with_text_content("Client pinged"))
            }

            "query_client_roots" => {
                let mut ctx = context.clone();
                let roots = ctx.list_roots().await?;
                Ok(CallToolResult::new()
                    .with_text_content(format!("Found {} client roots", roots.roots.len())))
            }

            "ask_client_to_generate" => {
                let params = CreateMessageParams {
                    messages: vec![SamplingMessage {
                        role: Role::User,
                        content: SamplingContent::Text(TextContent {
                            text: "Server request".to_string(),
                            annotations: None,
                        }),
                    }],
                    system_prompt: None,
                    include_context: None,
                    temperature: None,
                    max_tokens: 100,
                    metadata: None,
                    stop_sequences: None,
                    model_preferences: None,
                };

                let mut ctx = context.clone();
                let result = ctx.create_message(params).await?;
                match result.content {
                    SamplingContent::Text(text) => {
                        Ok(CallToolResult::new().with_text_content(text.text))
                    }
                    _ => Ok(CallToolResult::new().with_text_content("Non-text response")),
                }
            }

            _ => Err(tenx_mcp::Error::ToolExecutionFailed {
                tool: name,
                message: "Unknown tool".to_string(),
            }),
        }
    }

    async fn tools_list(
        &self,
        _context: &ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListToolsResult> {
        Ok(ListToolsResult::new()
            .with_tool(
                Tool::new("ping_client", ToolInputSchema::default())
                    .with_description("Ping the client during request handling"),
            )
            .with_tool(
                Tool::new("query_client_roots", ToolInputSchema::default())
                    .with_description("Query client's file roots during request handling"),
            )
            .with_tool(
                Tool::new("ask_client_to_generate", ToolInputSchema::default())
                    .with_description("Ask client to generate a message during request handling"),
            ))
    }
}

#[tokio::test]
async fn test_server_calls_client_during_request() {
    let _ = tracing_subscriber::fmt::try_init();

    // Create test client and server with call tracking
    let (test_client, client_calls) = TestClient::new();
    let (test_server, server_calls) = TestServer::new();

    let (mut client, server_handle) =
        connected_client_and_server_with_conn(move || Box::new(test_server.clone()), test_client)
            .await
            .expect("Failed to create client/server pair");

    // Initialize connection
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

    // Test 1: Server pings client during tool execution
    client_calls.lock().unwrap().clear();
    client
        .call_tool("ping_client", None)
        .await
        .expect("ping_client tool failed");

    assert_eq!(
        client_calls.lock().unwrap().as_slice(),
        &["client_pong"],
        "Server should have pinged client"
    );

    // Test 2: Server queries client roots during tool execution
    client_calls.lock().unwrap().clear();
    let result = client
        .call_tool("query_client_roots", None)
        .await
        .expect("query_client_roots tool failed");

    assert_eq!(
        client_calls.lock().unwrap().as_slice(),
        &["list_roots"],
        "Server should have queried client roots"
    );

    if let Some(Content::Text(text)) = result.content.first() {
        assert!(text.text.contains("1 client roots"));
    }

    // Test 3: Server asks client to generate message during tool execution
    client_calls.lock().unwrap().clear();
    let result = client
        .call_tool("ask_client_to_generate", None)
        .await
        .expect("ask_client_to_generate tool failed");

    assert_eq!(
        client_calls.lock().unwrap().as_slice(),
        &["create_message"],
        "Server should have asked client to create message"
    );

    if let Some(Content::Text(text)) = result.content.first() {
        assert_eq!(text.text, "Client received: Server request");
    }

    // Verify server tracked all tool calls
    {
        let server_call_log = server_calls.lock().unwrap();
        assert_eq!(
            server_call_log.as_slice(),
            &[
                "tool_ping_client",
                "tool_query_client_roots",
                "tool_ask_client_to_generate"
            ],
            "Server should have tracked all tool calls"
        );
    }

    shutdown_client_and_server(client, server_handle).await;
}

#[tokio::test]
async fn test_client_server_ping_pong() {
    let _ = tracing_subscriber::fmt::try_init();

    let (test_client, client_calls) = TestClient::new();
    let (test_server, server_calls) = TestServer::new();

    let (mut client, server_handle) =
        connected_client_and_server_with_conn(move || Box::new(test_server.clone()), test_client)
            .await
            .expect("Failed to create client/server pair");

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

    // Client pings server (normal direction)
    client.ping().await.expect("Client->Server ping failed");
    assert!(
        server_calls
            .lock()
            .unwrap()
            .contains(&"server_pong".to_string()),
        "Server should respond to client ping"
    );

    // Server pings client (reverse direction via tool call)
    client_calls.lock().unwrap().clear();
    client
        .call_tool("ping_client", None)
        .await
        .expect("Server->Client ping failed");
    assert!(
        client_calls
            .lock()
            .unwrap()
            .contains(&"client_pong".to_string()),
        "Client should respond to server ping"
    );

    shutdown_client_and_server(client, server_handle).await;
}
