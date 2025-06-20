use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tenx_mcp::{
    schema::*,
    testutils::{connected_client_and_server_with_conn, shutdown_client_and_server},
    ClientAPI, ClientConn, ClientCtx, Result, ServerAPI, ServerConn, ServerCtx,
};

/// Test client connection that tracks method calls
#[derive(Clone)]
struct TestClientConnection {
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ClientConn for TestClientConnection {
    async fn on_connect(&self, _context: ClientCtx) -> Result<()> {
        self.calls.lock().unwrap().push("on_connect".to_string());
        Ok(())
    }

    async fn on_disconnect(&self, _context: ClientCtx) -> Result<()> {
        self.calls.lock().unwrap().push("on_disconnect".to_string());
        Ok(())
    }

    async fn pong(&self, _context: ClientCtx) -> Result<()> {
        self.calls.lock().unwrap().push("client_pong".to_string());
        println!("Client received ping from server!");
        Ok(())
    }

    async fn create_message(
        &self,
        _context: ClientCtx,
        _method: &str,
        params: CreateMessageParams,
    ) -> Result<CreateMessageResult> {
        self.calls
            .lock()
            .unwrap()
            .push("create_message".to_string());

        // Extract the text from the first message
        let message_text = if let Some(first_msg) = params.messages.first() {
            if let SamplingContent::Text(text_content) = &first_msg.content {
                text_content.text.clone()
            } else {
                "No text content".to_string()
            }
        } else {
            "No messages".to_string()
        };

        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: SamplingContent::Text(TextContent {
                text: format!("Server asked me to respond to: {message_text}"),
                annotations: None,
            }),
            model: "test-model".to_string(),
            stop_reason: None,
            meta: None,
        })
    }

    async fn list_roots(&self, _context: ClientCtx) -> Result<ListRootsResult> {
        self.calls.lock().unwrap().push("list_roots".to_string());
        Ok(ListRootsResult {
            roots: vec![
                Root {
                    uri: "file:///home/user".to_string(),
                    name: Some("Home Directory".to_string()),
                },
                Root {
                    uri: "file:///tmp".to_string(),
                    name: Some("Temp Directory".to_string()),
                },
            ],
            meta: None,
        })
    }
}

/// Test server connection that makes requests back to the client
struct TestServerConnection {
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ServerConn for TestServerConnection {
    async fn on_connect(&self, _context: ServerCtx) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push("server_on_connect".to_string());
        Ok(())
    }

    async fn initialize(
        &self,
        _context: ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        self.calls.lock().unwrap().push("initialize".to_string());
        Ok(InitializeResult::new("test-server", "1.0.0"))
    }

    async fn pong(&self, _context: ServerCtx) -> Result<()> {
        self.calls.lock().unwrap().push("server_pong".to_string());
        Ok(())
    }

    /// Custom tool that demonstrates server making requests to client
    async fn tools_call(
        &self,
        mut context: ServerCtx,
        name: String,
        arguments: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<CallToolResult> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("tools_call:{name}"));

        match name.as_str() {
            "ping_client" => {
                // Server pings the client
                context.ping().await?;
                Ok(CallToolResult {
                    content: vec![Content::Text(TextContent {
                        text: "Successfully pinged the client".to_string(),
                        annotations: None,
                    })],
                    is_error: None,
                    meta: None,
                })
            }
            "ask_client_question" => {
                // Server asks client to create a message
                let question = arguments
                    .as_ref()
                    .and_then(|args| args.get("question"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("What is 2+2?");

                let params = CreateMessageParams {
                    messages: vec![SamplingMessage {
                        role: Role::User,
                        content: SamplingContent::Text(TextContent {
                            text: question.to_string(),
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

                let response = context.create_message(params).await?;

                let response_text = if let SamplingContent::Text(text_content) = &response.content {
                    text_content.text.clone()
                } else {
                    "No text in response".to_string()
                };

                Ok(CallToolResult {
                    content: vec![Content::Text(TextContent {
                        text: format!("Client responded: {response_text}"),
                        annotations: None,
                    })],
                    is_error: None,
                    meta: None,
                })
            }
            "get_client_roots" => {
                // Server asks client for available roots
                let roots = context.list_roots().await?;

                let roots_text = roots
                    .roots
                    .iter()
                    .map(|root| {
                        format!(
                            "- {} ({})",
                            root.name.as_ref().unwrap_or(&"Unnamed".to_string()),
                            root.uri
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                Ok(CallToolResult {
                    content: vec![Content::Text(TextContent {
                        text: format!("Client has {} roots:\n{}", roots.roots.len(), roots_text),
                        annotations: None,
                    })],
                    is_error: None,
                    meta: None,
                })
            }
            _ => Ok(CallToolResult {
                content: vec![Content::Text(TextContent {
                    text: format!("Unknown tool: {name}"),
                    annotations: None,
                })],
                is_error: Some(true),
                meta: None,
            }),
        }
    }

    async fn tools_list(
        &self,
        _context: ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListToolsResult> {
        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: "ping_client".to_string(),
                    description: Some("Ping the client to check connectivity".to_string()),
                    input_schema: ToolInputSchema {
                        schema_type: "object".to_string(),
                        properties: Some(std::collections::HashMap::new()),
                        required: None,
                    },
                    annotations: None,
                },
                Tool {
                    name: "ask_client_question".to_string(),
                    description: Some("Ask the client to generate a message response".to_string()),
                    input_schema: ToolInputSchema {
                        schema_type: "object".to_string(),
                        properties: Some({
                            let mut props = std::collections::HashMap::new();
                            props.insert(
                                "question".to_string(),
                                serde_json::json!({
                                    "type": "string",
                                    "description": "The question to ask the client"
                                }),
                            );
                            props
                        }),
                        required: Some(vec!["question".to_string()]),
                    },
                    annotations: None,
                },
                Tool {
                    name: "get_client_roots".to_string(),
                    description: Some("Get available filesystem roots from the client".to_string()),
                    input_schema: ToolInputSchema {
                        schema_type: "object".to_string(),
                        properties: Some(std::collections::HashMap::new()),
                        required: None,
                    },
                    annotations: None,
                },
            ],
            next_cursor: None,
        })
    }
}

#[tokio::test]
async fn test_server_makes_requests_to_client() {
    // Install tracing subscriber for debugging
    let _ = tracing_subscriber::fmt::try_init();

    // Shared state to track method calls
    let client_calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let server_calls = Arc::new(Mutex::new(Vec::<String>::new()));

    // Create client and server
    let (mut client, server_handle) = connected_client_and_server_with_conn(
        {
            let server_calls = server_calls.clone();
            move || {
                Box::new(TestServerConnection {
                    calls: server_calls.clone(),
                })
            }
        },
        TestClientConnection {
            calls: client_calls.clone(),
        },
    )
    .await
    .expect("failed to set up test client/server pair");

    // Initialize the connection
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

    // Test 1: Server pings client
    println!("\n=== Test 1: Server pings client ===");
    client_calls.lock().unwrap().clear();

    let result = client
        .call_tool("ping_client", None)
        .await
        .expect("Tool call failed");

    println!("Tool result: {result:?}");
    assert!(client_calls
        .lock()
        .unwrap()
        .contains(&"client_pong".to_string()));

    // Test 2: Server asks client a question
    println!("\n=== Test 2: Server asks client a question ===");
    client_calls.lock().unwrap().clear();

    let mut args = std::collections::HashMap::new();
    args.insert(
        "question".to_string(),
        serde_json::json!("What is the meaning of life?"),
    );

    let result = client
        .call_tool("ask_client_question", Some(args))
        .await
        .expect("Tool call failed");

    println!("Tool result: {result:?}");
    assert!(client_calls
        .lock()
        .unwrap()
        .contains(&"create_message".to_string()));

    // Test 3: Server gets client roots
    println!("\n=== Test 3: Server gets client roots ===");
    client_calls.lock().unwrap().clear();

    let result = client
        .call_tool("get_client_roots", None)
        .await
        .expect("Tool call failed");

    println!("Tool result: {result:?}");
    assert!(client_calls
        .lock()
        .unwrap()
        .contains(&"list_roots".to_string()));

    // Clean up
    shutdown_client_and_server(client, server_handle).await;
}

#[tokio::test]
async fn test_server_ctx_client_api_methods() {
    // Direct unit test of ServerCtx ClientAPI implementation

    // Note: ServerCtx::new is pub(crate), so we can't directly test it
    // In a real scenario, the ServerCtx would be provided by the server framework
    println!("Note: ServerCtx::new is not publicly accessible.");
    println!("The ServerCtx would be provided to ServerConn methods by the framework.");

    println!("\nServerCtx implements the ClientAPI trait with these methods:");
    println!("  - ping(): Send a ping request to the client");
    println!("  - create_message(): Ask the client to generate a message");
    println!("  - list_roots(): Get available filesystem roots from the client");
    println!("\nThese methods are available within ServerConn trait methods.");
}
