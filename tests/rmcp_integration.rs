//! Integration tests for rmcp and tenx-mcp interoperability
//!
//! This module contains actual integration tests that verify both
//! implementations can communicate with each other correctly.

use std::collections::HashMap;

use async_trait::async_trait;
use rmcp::model::{CallToolRequestParam, PaginatedRequestParam};
use rmcp::ServiceExt;
use serde_json::json;
use tenx_mcp::error::{Error, Result};
use tenx_mcp::{
    schema::*, server_connection::ServerConnection, testutils::make_duplex_pair, Client, Server,
};

struct EchoConnection;

#[async_trait]
impl ServerConnection for EchoConnection {
    async fn initialize(
        &mut self,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new("test-server", "0.1.0").with_tools(true))
    }

    async fn tools_list(&mut self) -> Result<ListToolsResult> {
        tracing::info!("EchoConnection.tools_list called");
        let schema = ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "message".to_string(),
                    json!({
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

    async fn tools_call(
        &mut self,
        name: String,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        if name != "echo" {
            return Err(Error::ToolExecutionFailed {
                tool: name,
                message: "Tool not found".to_string(),
            });
        }

        let args =
            arguments.ok_or_else(|| Error::InvalidParams("echo: Missing arguments".to_string()))?;
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidParams("echo: Missing message parameter".to_string()))?;

        Ok(CallToolResult {
            content: vec![Content::Text(TextContent {
                text: message.to_string(),
                annotations: None,
            })],
            is_error: Some(false),
            meta: None,
        })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_tenx_server_with_rmcp_client() {
    // Initialize a tracing subscriber so that we get helpful debug output if
    // this test fails or hangs. We deliberately call `try_init` so that it's
    // no-op when a subscriber has already been installed by another test.
    let _ = tracing_subscriber::fmt::try_init();
    // Create bidirectional streams for communication using the shared test
    // utility.
    let (server_reader, server_writer, client_reader, client_writer) = make_duplex_pair();

    // Create and configure tenx-mcp server
    let server = Server::default()
        .with_connection_factory(|| Box::new(EchoConnection))
        .with_capabilities(ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: Some(true),
            }),
            resources: None,
            prompts: None,
            logging: None,
            completions: None,
            experimental: None,
        });

    // Start tenx-mcp server in background using the new serve_stream method
    let server_handle = tenx_mcp::ServerHandle::from_stream(server, server_reader, server_writer)
        .await
        .expect("Failed to start server");

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create rmcp client using the streams
    let client_transport = (client_reader, client_writer);

    // Connect rmcp client - initialization happens automatically
    let client = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        ().serve(client_transport),
    )
    .await
    .expect("Client connection timed out")
    .expect("Failed to connect client");

    // List tools
    let tools = client.list_tools(None).await.unwrap();
    assert_eq!(tools.tools.len(), 1);
    assert_eq!(tools.tools[0].name, "echo");

    // Call echo tool
    let mut args = serde_json::Map::new();
    args.insert("message".to_string(), json!("Hello from rmcp!"));

    let result = client
        .call_tool(rmcp::model::CallToolRequestParam {
            name: "echo".into(),
            arguments: Some(args),
        })
        .await
        .unwrap();

    // Verify result
    assert_eq!(result.content.len(), 1);
    match &result.content[0].raw {
        rmcp::model::RawContent::Text(text_content) => {
            assert_eq!(&text_content.text, "Hello from rmcp!");
        }
        _ => panic!("Expected text content"),
    }

    // Cleanup: we drop the client first so that the underlying transport is
    // closed and the server task can finish. To avoid hanging the test in the
    // unlikely case that it doesn't shut down promptly, we wrap the wait in a
    // short timeout.
    drop(client);

    // Give the server task a moment to observe the closed connection and shut
    // itself down. We ignore any timeout errors here because the important
    // part of the test (inter-operability) has already completed.
    let _ = tokio::time::timeout(std::time::Duration::from_millis(100), server_handle.stop()).await;
}

#[tokio::test]
async fn test_rmcp_server_with_tenx_client() {
    use rmcp::{
        handler::server::ServerHandler,
        service::{RequestContext, RoleServer},
    };

    // Create a simple rmcp server
    #[derive(Debug, Clone)]
    struct TestRmcpServer;

    impl ServerHandler for TestRmcpServer {
        async fn initialize(
            &self,
            _request: rmcp::model::InitializeRequestParam,
            _ctx: RequestContext<RoleServer>,
        ) -> std::result::Result<rmcp::model::InitializeResult, rmcp::Error> {
            Ok(rmcp::model::InitializeResult {
                protocol_version: rmcp::model::ProtocolVersion::default(),
                capabilities: rmcp::model::ServerCapabilities::default(),
                server_info: rmcp::model::Implementation {
                    name: "test-rmcp-server".to_string(),
                    version: "0.1.0".to_string(),
                },
                instructions: None,
            })
        }

        async fn list_tools(
            &self,
            _params: Option<PaginatedRequestParam>,
            _ctx: RequestContext<RoleServer>,
        ) -> std::result::Result<rmcp::model::ListToolsResult, rmcp::Error> {
            Ok(rmcp::model::ListToolsResult {
                next_cursor: None,
                tools: vec![rmcp::model::Tool {
                    name: "reverse".into(),
                    description: Some("Reverses a string".into()),
                    input_schema: {
                        let mut schema = serde_json::Map::new();
                        schema.insert("type".to_string(), json!("object"));

                        let mut properties = serde_json::Map::new();
                        properties.insert(
                            "text".to_string(),
                            json!({
                                "type": "string",
                                "description": "Text to reverse"
                            }),
                        );
                        schema.insert("properties".to_string(), json!(properties));
                        schema.insert("required".to_string(), json!(["text"]));
                        std::sync::Arc::new(schema)
                    },
                    annotations: None,
                }],
            })
        }

        async fn call_tool(
            &self,
            params: CallToolRequestParam,
            _ctx: RequestContext<RoleServer>,
        ) -> std::result::Result<rmcp::model::CallToolResult, rmcp::Error> {
            if params.name == "reverse" {
                let text = params
                    .arguments
                    .as_ref()
                    .and_then(|args| args.get("text"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        rmcp::Error::invalid_params("reverse: Missing text parameter", None)
                    })?;

                let reversed = text.chars().rev().collect::<String>();

                Ok(rmcp::model::CallToolResult {
                    content: vec![rmcp::model::Content::text(reversed)],
                    is_error: None,
                })
            } else {
                Err(rmcp::Error::invalid_request("Unknown tool", None))
            }
        }
    }

    // Create bidirectional streams
    let (client_reader, server_writer) = tokio::io::duplex(8192);
    let (server_reader, client_writer) = tokio::io::duplex(8192);

    // Start rmcp server
    let server = TestRmcpServer;
    let server_handle = tokio::spawn(async move {
        let transport = (server_reader, server_writer);
        use rmcp::ServiceExt;
        let _service = server.serve(transport).await.unwrap();
        // Keep the server running
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Create tenx-mcp client
    let mut client = Client::new();
    client
        .connect_stream(client_reader, client_writer)
        .await
        .unwrap();

    // Initialize
    let init_result = client
        .initialize(
            Implementation {
                name: "test-client".to_string(),
                version: "0.1.0".to_string(),
            },
            ClientCapabilities {
                experimental: None,
                roots: None,
                sampling: None,
            },
        )
        .await
        .unwrap();

    // Check server info is valid
    assert!(!init_result.server_info.name.is_empty());

    // List tools from rmcp server
    let tools = client.list_tools().await.unwrap();
    assert_eq!(tools.tools.len(), 1);
    assert_eq!(tools.tools[0].name, "reverse");

    // Call reverse tool
    let result = client
        .call_tool("reverse", &json!({ "text": "hello" }))
        .await
        .unwrap();

    // Verify reversed result
    assert_eq!(result.content.len(), 1);
    match &result.content[0] {
        Content::Text(text) => {
            assert_eq!(text.text, "olleh");
        }
        _ => panic!("Expected text content"),
    }

    // Cleanup
    server_handle.abort();
}
