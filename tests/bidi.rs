/// Test bidirectional client-server and server-client messages
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tenx_mcp::{
    schema::*,
    testutils::{connected_client_and_server_with_conn, shutdown_client_and_server},
    ClientAPI, ClientConn, ClientCtx, Result, ServerAPI, ServerConn, ServerCtx,
};

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

    async fn list_roots(&self, context: ClientCtx) -> Result<ListRootsResult> {
        self.calls.lock().unwrap().push("list_roots".to_string());

        // Client sends notification to server during request handling
        context.send_notification(ClientNotification::RootsListChanged)?;

        Ok(ListRootsResult {
            roots: vec![Root {
                uri: "file:///test".to_string(),
                name: Some("Test Root".to_string()),
            }],
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

    async fn notification(
        &self,
        _context: ServerCtx,
        notification: ClientNotification,
    ) -> Result<()> {
        if let ClientNotification::RootsListChanged = notification {
            self.calls.lock().unwrap().push("roots_changed".to_string());
        }
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

    // Test server getting roots from client (and client notifying server)
    client_calls.lock().unwrap().clear();
    server_calls.lock().unwrap().clear();
    client
        .call_tool("test_roots", None)
        .await
        .expect("Tool call failed");

    // Small delay to ensure notification is processed
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    assert!(client_calls
        .lock()
        .unwrap()
        .contains(&"list_roots".to_string()));
    assert!(server_calls
        .lock()
        .unwrap()
        .contains(&"roots_changed".to_string()));

    shutdown_client_and_server(client, server_handle).await;
}
