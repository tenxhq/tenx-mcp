//! Unit tests for JSON-RPC error handling
//!
//! These tests focus on testing error handling at the Connection level
//! without the complexity of setting up full client-server communication.

use std::collections::HashMap;
use tenx_mcp::{schema, Error, Result, ServerConnection, ServerConnectionContext};

fn create_test_context() -> ServerConnectionContext {
    let (notification_tx, _) = tokio::sync::broadcast::channel(100);
    ServerConnectionContext::new(notification_tx)
}

#[tokio::test]
async fn test_method_not_found() {
    // Test that the default tools_call implementation returns ToolNotFound
    #[derive(Default)]
    struct MinimalConnection;

    #[async_trait::async_trait]
    impl ServerConnection for MinimalConnection {
        async fn initialize(
            &mut self,
            _context: ServerConnectionContext,
            _protocol_version: String,
            _capabilities: schema::ClientCapabilities,
            _client_info: schema::Implementation,
        ) -> Result<schema::InitializeResult> {
            Ok(schema::InitializeResult {
                protocol_version: schema::LATEST_PROTOCOL_VERSION.to_string(),
                capabilities: schema::ServerCapabilities::default(),
                server_info: schema::Implementation {
                    name: "test-server".to_string(),
                    version: "1.0.0".to_string(),
                },
                instructions: None,
                meta: None,
            })
        }
    }

    let mut conn = MinimalConnection;

    // Call a tool on a connection that doesn't implement tools_call
    // This should use the default implementation which returns ToolNotFound
    let context = create_test_context();
    let result = conn
        .tools_call(context, "non_existent".to_string(), None)
        .await;

    assert!(result.is_err());
    match result {
        Err(Error::ToolExecutionFailed { tool, message }) => {
            assert_eq!(tool, "non_existent");
            assert!(message.contains("not found"), "Message was: {message}");
        }
        _ => panic!("unexpected result: {result:?}"),
    }
}

#[tokio::test]
async fn test_invalid_params() {
    // Test parameter validation in tools_call
    #[derive(Default)]
    struct ConnectionWithValidation;

    #[async_trait::async_trait]
    impl ServerConnection for ConnectionWithValidation {
        async fn initialize(
            &mut self,
            _context: ServerConnectionContext,
            _protocol_version: String,
            _capabilities: schema::ClientCapabilities,
            _client_info: schema::Implementation,
        ) -> Result<schema::InitializeResult> {
            Ok(schema::InitializeResult {
                protocol_version: schema::LATEST_PROTOCOL_VERSION.to_string(),
                capabilities: schema::ServerCapabilities {
                    tools: Some(schema::ToolsCapability { list_changed: None }),
                    ..Default::default()
                },
                server_info: schema::Implementation {
                    name: "test-server".to_string(),
                    version: "1.0.0".to_string(),
                },
                instructions: None,
                meta: None,
            })
        }

        async fn tools_list(
            &mut self,
            _context: ServerConnectionContext,
        ) -> Result<schema::ListToolsResult> {
            let schema = schema::ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some({
                    let mut props = HashMap::new();
                    props.insert(
                        "required_param".to_string(),
                        serde_json::json!({
                            "type": "string",
                            "description": "A required parameter"
                        }),
                    );
                    props
                }),
                required: Some(vec!["required_param".to_string()]),
            };

            Ok(schema::ListToolsResult::new().with_tool(
                schema::Tool::new("test_tool", schema)
                    .with_description("A test tool that requires a parameter"),
            ))
        }

        async fn tools_call(
            &mut self,
            _context: ServerConnectionContext,
            name: String,
            arguments: Option<serde_json::Value>,
        ) -> Result<schema::CallToolResult> {
            if name != "test_tool" {
                return Err(Error::ToolNotFound(name));
            }

            // Validate arguments
            let args =
                arguments.ok_or_else(|| Error::InvalidParams("Missing arguments".to_string()))?;

            let args_obj = args
                .as_object()
                .ok_or_else(|| Error::InvalidParams("Arguments must be an object".to_string()))?;

            if !args_obj.contains_key("required_param") {
                return Err(Error::InvalidParams("Missing required_param".to_string()));
            }

            Ok(schema::CallToolResult::new()
                .with_text_content("Success")
                .is_error(false))
        }
    }

    let mut conn = ConnectionWithValidation;

    // Test 1: Call with missing arguments
    let context = create_test_context();
    let result = conn
        .tools_call(context, "test_tool".to_string(), None)
        .await;
    assert!(matches!(result, Err(Error::InvalidParams(_))));

    // Test 2: Call with empty object (missing required param)
    let context = create_test_context();
    let result = conn
        .tools_call(
            context,
            "test_tool".to_string(),
            Some(serde_json::json!({})),
        )
        .await;
    match result {
        Err(Error::InvalidParams(msg)) => {
            assert!(msg.contains("required_param"), "Error was: {msg}");
        }
        _ => panic!("Expected InvalidParams error"),
    }

    // Test 3: Call with correct parameters should succeed
    let context = create_test_context();
    let result = conn
        .tools_call(
            context,
            "test_tool".to_string(),
            Some(serde_json::json!({ "required_param": "test" })),
        )
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_successful_response() {
    // Test successful tool listing and other operations
    #[derive(Default)]
    struct ConnectionWithTools;

    #[async_trait::async_trait]
    impl ServerConnection for ConnectionWithTools {
        async fn initialize(
            &mut self,
            _context: ServerConnectionContext,
            _protocol_version: String,
            _capabilities: schema::ClientCapabilities,
            _client_info: schema::Implementation,
        ) -> Result<schema::InitializeResult> {
            Ok(schema::InitializeResult {
                protocol_version: schema::LATEST_PROTOCOL_VERSION.to_string(),
                capabilities: schema::ServerCapabilities {
                    tools: Some(schema::ToolsCapability { list_changed: None }),
                    resources: Some(schema::ResourcesCapability {
                        subscribe: Some(true),
                        list_changed: None,
                    }),
                    ..Default::default()
                },
                server_info: schema::Implementation {
                    name: "test-server".to_string(),
                    version: "1.0.0".to_string(),
                },
                instructions: None,
                meta: None,
            })
        }

        async fn tools_list(
            &mut self,
            _context: ServerConnectionContext,
        ) -> Result<schema::ListToolsResult> {
            Ok(schema::ListToolsResult::new()
                .with_tool(
                    schema::Tool::new("echo", schema::ToolInputSchema::default())
                        .with_description("Echoes the input"),
                )
                .with_tool(
                    schema::Tool::new("add", schema::ToolInputSchema::default())
                        .with_description("Adds two numbers"),
                ))
        }

        async fn resources_list(
            &mut self,
            _context: ServerConnectionContext,
        ) -> Result<schema::ListResourcesResult> {
            Ok(schema::ListResourcesResult {
                resources: vec![schema::Resource {
                    uri: "file:///test.txt".to_string(),
                    name: "test.txt".to_string(),
                    description: Some("A test file".to_string()),
                    mime_type: Some("text/plain".to_string()),
                    size: None,
                    annotations: None,
                }],
                next_cursor: None,
            })
        }
    }

    let mut conn = ConnectionWithTools;

    // Test successful initialization
    let context = create_test_context();
    let init_result = conn
        .initialize(
            context,
            schema::LATEST_PROTOCOL_VERSION.to_string(),
            schema::ClientCapabilities::default(),
            schema::Implementation {
                name: "test-client".to_string(),
                version: "1.0.0".to_string(),
            },
        )
        .await
        .unwrap();

    assert_eq!(init_result.server_info.name, "test-server");
    assert!(init_result.capabilities.tools.is_some());
    assert!(init_result.capabilities.resources.is_some());

    // Test successful tools listing
    let context = create_test_context();
    let tools = conn.tools_list(context).await.unwrap();
    assert_eq!(tools.tools.len(), 2);
    assert_eq!(tools.tools[0].name, "echo");
    assert_eq!(tools.tools[1].name, "add");

    // Test successful resources listing
    let context = create_test_context();
    let resources = conn.resources_list(context).await.unwrap();
    assert_eq!(resources.resources.len(), 1);
    assert_eq!(resources.resources[0].uri, "file:///test.txt");
}

#[tokio::test]
async fn test_error_propagation() {
    // Test that errors are properly propagated through the Connection trait
    #[derive(Default)]
    struct FaultyConnection;

    #[async_trait::async_trait]
    impl ServerConnection for FaultyConnection {
        async fn initialize(
            &mut self,
            _context: ServerConnectionContext,
            _protocol_version: String,
            _capabilities: schema::ClientCapabilities,
            _client_info: schema::Implementation,
        ) -> Result<schema::InitializeResult> {
            // Simulate an internal error during initialization
            Err(Error::InternalError("Connection failed".to_string()))
        }

        async fn resources_read(
            &mut self,
            _context: ServerConnectionContext,
            uri: String,
        ) -> Result<schema::ReadResourceResult> {
            // Simulate resource not found
            Err(Error::ResourceNotFound { uri })
        }

        async fn prompts_get(
            &mut self,
            _context: ServerConnectionContext,
            name: String,
            _arguments: Option<HashMap<String, serde_json::Value>>,
        ) -> Result<schema::GetPromptResult> {
            // Simulate prompt not found - using MethodNotFound as PromptNotFound doesn't exist
            Err(Error::MethodNotFound(format!("prompt/{name}")))
        }
    }

    let mut conn = FaultyConnection;

    // Test initialization error
    let context = create_test_context();
    let init_result = conn
        .initialize(
            context,
            schema::LATEST_PROTOCOL_VERSION.to_string(),
            schema::ClientCapabilities::default(),
            schema::Implementation {
                name: "test-client".to_string(),
                version: "1.0.0".to_string(),
            },
        )
        .await;

    match init_result {
        Err(Error::InternalError(msg)) => {
            assert!(msg.contains("Connection failed"));
        }
        _ => panic!("Expected InternalError"),
    }

    // Test resource not found
    let context = create_test_context();
    let read_result = conn
        .resources_read(context, "file:///missing.txt".to_string())
        .await;
    match read_result {
        Err(Error::ResourceNotFound { uri }) => {
            assert_eq!(uri, "file:///missing.txt");
        }
        _ => panic!("Expected ResourceNotFound error"),
    }

    // Test prompt not found (using MethodNotFound)
    let context = create_test_context();
    let prompt_result = conn
        .prompts_get(context, "missing_prompt".to_string(), None)
        .await;
    match prompt_result {
        Err(Error::MethodNotFound(method)) => {
            assert!(method.contains("missing_prompt"));
        }
        _ => panic!("Expected MethodNotFound error"),
    }
}
