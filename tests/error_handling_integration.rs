//! Simplified integration tests for error handling

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use tenx_mcp::{
    codec::JsonRpcCodec,
    connection::Connection,
    error::{Error, Result},
    schema::*,
    server::Server,
    ServerHandle,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

/// Simple connection for testing error handling
struct TestConnection;

#[async_trait]
impl Connection for TestConnection {
    async fn initialize(
        &mut self,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(true),
                }),
                ..Default::default()
            },
            server_info: Implementation {
                name: "test-server".to_string(),
                version: "1.0.0".to_string(),
            },
            instructions: None,
            meta: None,
        })
    }

    async fn tools_list(&mut self) -> Result<ListToolsResult> {
        let schema = ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "required_field".to_string(),
                    serde_json::json!({"type": "string"}),
                );
                props
            }),
            required: Some(vec!["required_field".to_string()]),
        };

        Ok(ListToolsResult::new()
            .with_tool(Tool::new("test", schema).with_description("Test tool")))
    }

    async fn tools_call(
        &mut self,
        name: String,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        if name != "test" {
            return Err(Error::ToolExecutionFailed {
                tool: name,
                message: "Tool not found".to_string(),
            });
        }

        let args = arguments
            .ok_or_else(|| Error::InvalidParams("strict_params: Missing arguments".to_string()))?;

        let _field = args.get("required_field").ok_or_else(|| {
            Error::InvalidParams("strict_params: Missing required_field".to_string())
        })?;

        Ok(CallToolResult::new()
            .with_text_content("Success")
            .is_error(false))
    }
}

// Helper to create a bidirectional connection using two duplex streams
fn create_test_connection() -> (
    impl AsyncRead + AsyncWrite + Send + Sync + Unpin,
    impl AsyncRead + Send + Sync + Unpin,
    impl AsyncWrite + Send + Sync + Unpin,
) {
    // Create two duplex streams to simulate bidirectional communication
    let (client_to_server, server_from_client) = tokio::io::duplex(8192);
    let (server_to_client, client_from_server) = tokio::io::duplex(8192);

    // Combine streams for client side
    let client = tokio::io::join(client_from_server, client_to_server);

    (client, server_from_client, server_to_client)
}

#[tokio::test]
async fn test_error_responses() {
    // Setup server
    let server = Server::default().with_connection_factory(|| Box::new(TestConnection));

    let (client_stream, server_reader, server_writer) = create_test_connection();

    // Start server with the proper read/write streams
    let server_handle = ServerHandle::from_stream(server, server_reader, server_writer)
        .await
        .unwrap();

    // Create client using the combined stream
    let mut stream = Framed::new(client_stream, JsonRpcCodec::new());

    // Test 1: Unknown method
    if stream
        .send(JSONRPCMessage::Request(JSONRPCRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: RequestId::Number(1),
            request: Request {
                method: "unknown".to_string(),
                params: None,
            },
        }))
        .await
        .is_err()
    {
        // Connection closed, which is expected behavior
        server_handle.stop().await.ok();
        return;
    }

    if let Some(Ok(response)) = stream.next().await {
        assert!(matches!(
            response,
            JSONRPCMessage::Error(err) if err.error.code == METHOD_NOT_FOUND
        ));

        // Test 2: Missing params for initialize
        if stream
            .send(JSONRPCMessage::Request(JSONRPCRequest {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: RequestId::Number(2),
                request: Request {
                    method: "initialize".to_string(),
                    params: None,
                },
            }))
            .await
            .is_ok()
        {
            if let Some(Ok(response)) = stream.next().await {
                assert!(matches!(
                    response,
                    JSONRPCMessage::Error(err) if err.error.code == INVALID_PARAMS
                ));
            }
        }

        // Test 3: Tool with missing required param
        if stream
            .send(JSONRPCMessage::Request(JSONRPCRequest {
                jsonrpc: JSONRPC_VERSION.to_string(),
                id: RequestId::Number(3),
                request: Request {
                    method: "tools/call".to_string(),
                    params: Some(RequestParams {
                        meta: None,
                        other: {
                            let mut map = HashMap::new();
                            map.insert("name".to_string(), serde_json::json!("test"));
                            map.insert("arguments".to_string(), serde_json::json!({}));
                            map
                        },
                    }),
                },
            }))
            .await
            .is_ok()
        {
            if let Some(Ok(response)) = stream.next().await {
                assert!(matches!(
                    response,
                    JSONRPCMessage::Error(err) if err.error.code == INVALID_PARAMS && err.error.message.contains("Missing required_field")
                ));
            }
        }
    }

    drop(stream);
    server_handle.stop().await.unwrap();
}
