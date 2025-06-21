/// Test bidirectional client-server and server-client messages
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tenx_mcp::{
    schema::*,
    testutils::{connected_client_and_server_with_conn, shutdown_client_and_server},
    ClientAPI, ClientConn, ClientCtx, Result, ServerAPI, ServerConn, ServerCtx,
};
use tracing::info;

type CallLog = Arc<Mutex<Vec<String>>>;

/// Simple test client that responds to server requests
#[derive(Clone)]
struct SimpleClient {
    calls: CallLog,
}

#[async_trait]
impl ClientConn for SimpleClient {
    async fn pong(&self, _context: ClientCtx) -> Result<()> {
        self.calls.lock().unwrap().push("client_pong".to_string());
        Ok(())
    }

    async fn list_roots(&self, _context: ClientCtx) -> Result<ListRootsResult> {
        info!("Client: list_roots called");
        self.calls.lock().unwrap().push("list_roots".to_string());

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
        _context: ClientCtx,
        _method: &str,
        _params: CreateMessageParams,
    ) -> Result<CreateMessageResult> {
        info!("Client: create_message called");
        self.calls
            .lock()
            .unwrap()
            .push("create_message".to_string());

        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: SamplingContent::Text(TextContent {
                text: "Test message from client".to_string(),
                annotations: None,
            }),
            model: "test-model".to_string(),
            stop_reason: None,
            meta: None,
        })
    }
}

type StoredContext = Arc<Mutex<Option<ServerCtx>>>;

/// Simple test server that makes requests to the client
struct SimpleServer {
    stored_context: StoredContext,
    calls: CallLog,
}

#[async_trait]
impl ServerConn for SimpleServer {
    async fn on_connect(&self, context: ServerCtx) -> Result<()> {
        *self.stored_context.lock().unwrap() = Some(context);
        Ok(())
    }

    async fn pong(&self, _context: ServerCtx) -> Result<()> {
        self.calls.lock().unwrap().push("server_pong".to_string());
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

    async fn tools_call(
        &self,
        mut context: ServerCtx,
        name: String,
        _arguments: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<CallToolResult> {
        match name.as_str() {
            "test_ping" => {
                context.ping().await?;
                Ok(CallToolResult::new().with_text_content("Ping successful"))
            }
            "test_roots" => {
                let roots = context.list_roots().await?;
                let text = format!("Client has {} roots", roots.roots.len());
                Ok(CallToolResult::new().with_text_content(text))
            }
            "test_create_message" => {
                // Server calls client's create_message method
                let params = CreateMessageParams {
                    messages: vec![SamplingMessage {
                        role: Role::User,
                        content: SamplingContent::Text(TextContent {
                            text: "Hello from server".to_string(),
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
                let result = context.create_message(params).await?;
                if let SamplingContent::Text(text_content) = result.content {
                    Ok(CallToolResult::new()
                        .with_text_content(format!("Client responded: {}", text_content.text)))
                } else {
                    Ok(CallToolResult::new()
                        .with_text_content("Client responded with non-text content"))
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
            )
            .with_tool(
                Tool::new("test_create_message", ToolInputSchema::default())
                    .with_description("Test server->client create_message"),
            ))
    }
}

#[tokio::test]
async fn test_bidirectional_communication() {
    let _ = tracing_subscriber::fmt::try_init();

    let client_calls = CallLog::default();
    let server_calls = CallLog::default();
    let server_context = StoredContext::default();

    let (mut client, server_handle) = connected_client_and_server_with_conn(
        {
            let ctx = server_context.clone();
            let calls = server_calls.clone();
            move || {
                Box::new(SimpleServer {
                    stored_context: ctx.clone(),
                    calls: calls.clone(),
                })
            }
        },
        SimpleClient {
            calls: client_calls.clone(),
        },
    )
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

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Test server pinging client
    client_calls.lock().unwrap().clear();
    client
        .call_tool("test_ping", None)
        .await
        .expect("Tool call failed");
    assert!(client_calls
        .lock()
        .unwrap()
        .contains(&"client_pong".to_string()));

    // Test server getting roots from client
    client_calls.lock().unwrap().clear();
    client
        .call_tool("test_roots", None)
        .await
        .expect("Tool call failed");
    assert!(client_calls
        .lock()
        .unwrap()
        .contains(&"list_roots".to_string()));

    // Test server calling client's create_message method
    client_calls.lock().unwrap().clear();
    let result = client
        .call_tool("test_create_message", None)
        .await
        .expect("Tool call failed");
    assert!(client_calls
        .lock()
        .unwrap()
        .contains(&"create_message".to_string()));

    // Verify the result contains the client's response
    if let Some(content) = result.content.first() {
        if let Content::Text(text_content) = content {
            assert!(text_content.text.contains("Client responded"));
        }
    }

    // The test demonstrates true bidirectional request/response messaging:
    // 1. Client -> Server: client.call_tool() sends requests to server
    // 2. Server -> Client: server can call ping, list_roots, and create_message on client
    // Both directions use full request/response patterns, not just notifications

    shutdown_client_and_server(client, server_handle).await;
}
