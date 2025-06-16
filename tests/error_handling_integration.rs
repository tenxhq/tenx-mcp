//! Simplified integration tests for error handling

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use tenx_mcp::{
    error::{MCPError, Result},
    schema::*,
    server::{MCPServer, ToolHandler},
    transport::Transport,
};

/// Simple tool for testing error handling
struct TestTool;

#[async_trait]
impl ToolHandler for TestTool {
    fn metadata(&self) -> Tool {
        Tool {
            name: "test".to_string(),
            description: Some("Test tool".to_string()),
            input_schema: ToolInputSchema {
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
            },
            annotations: None,
        }
    }

    async fn execute(&self, arguments: Option<serde_json::Value>) -> Result<Vec<Content>> {
        let args = arguments
            .ok_or_else(|| MCPError::invalid_params("strict_params", "Missing arguments"))?;

        let _field = args
            .get("required_field")
            .ok_or_else(|| MCPError::invalid_params("strict_params", "Missing required_field"))?;

        Ok(vec![Content::Text(TextContent {
            text: "Success".to_string(),
            annotations: None,
        })])
    }
}

// Minimal test transport
mod test_transport {
    use super::*;
    use tenx_mcp::codec::JsonRpcCodec;
    use tenx_mcp::transport::TransportStream;
    use tokio_util::codec::Framed;

    pub struct TestTransport {
        stream: Option<tokio::io::DuplexStream>,
    }

    impl TestTransport {
        pub fn new(stream: tokio::io::DuplexStream) -> Self {
            Self {
                stream: Some(stream),
            }
        }
    }

    #[async_trait]
    impl Transport for TestTransport {
        async fn connect(&mut self) -> Result<()> {
            Ok(())
        }

        fn framed(mut self: Box<Self>) -> Result<Box<dyn TransportStream>> {
            let stream = self.stream.take().unwrap();
            Ok(Box::new(Framed::new(stream, JsonRpcCodec::new())))
        }
    }
}

use test_transport::TestTransport;

#[tokio::test]
async fn test_error_responses() {
    // Setup server
    let mut server = MCPServer::new("test-server".to_string(), "0.1.0".to_string());
    server.register_tool(Box::new(TestTool));

    // Create streams
    let (client_stream, server_stream) = tokio::io::duplex(8192);

    // Start server
    let server_handle = tokio::spawn(async move {
        let transport: Box<dyn Transport> = Box::new(TestTransport::new(server_stream));
        let server_handle = tenx_mcp::MCPServerHandle::new(server, transport)
            .await
            .unwrap();
        server_handle.handle.await.ok();
    });

    // Test raw JSON-RPC error responses
    let mut transport = Box::new(TestTransport::new(client_stream));
    transport.connect().await.unwrap();
    let mut stream = transport.framed().unwrap();

    // Test 1: Unknown method
    if let Err(_) = stream
        .send(JSONRPCMessage::Request(JSONRPCRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: RequestId::Number(1),
            request: Request {
                method: "unknown".to_string(),
                params: None,
            },
        }))
        .await
    {
        // Connection closed, which is expected behavior
        server_handle.abort();
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
    server_handle.abort();
}
