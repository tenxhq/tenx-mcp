//! Integration tests for rmcp and tenx-mcp interoperability
//!
//! This module contains actual integration tests that verify both
//! implementations can communicate with each other correctly.

use std::collections::HashMap;

use async_trait::async_trait;
use rmcp::ServiceExt;
// Import rmcp types
use rmcp::model::{CallToolRequestParam, PaginatedRequestParam};
use serde_json::json;
// Import tenx-mcp types
use tenx_mcp::error::{MCPError, Result};
use tenx_mcp::{connection::Connection, schema::*, MCPClient, MCPServer};
use tokio::io::{AsyncRead, AsyncWrite};

// Simple echo connection for testing
struct EchoConnection;

#[async_trait]
impl Connection for EchoConnection {
    async fn initialize(
        &mut self,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new("test-server", "0.1.0").with_tools(true))
    }

    async fn tools_list(&mut self) -> Result<ListToolsResult> {
        Ok(ListToolsResult {
            tools: vec![Tool {
                name: "echo".to_string(),
                description: Some("Echoes the input message".to_string()),
                input_schema: ToolInputSchema {
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
                },
                annotations: None,
            }],
            next_cursor: None,
        })
    }

    async fn tools_call(
        &mut self,
        name: String,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        if name != "echo" {
            return Err(MCPError::ToolExecutionFailed {
                tool: name,
                message: "Tool not found".to_string(),
            });
        }

        let args =
            arguments.ok_or_else(|| MCPError::invalid_params("echo", "Missing arguments"))?;
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MCPError::invalid_params("echo", "Missing message parameter"))?;

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
#[ignore] // TODO: Fix integration with rmcp client
async fn test_tenx_server_with_rmcp_client() {
    // Create bidirectional streams for communication
    let (server_reader, client_writer) = tokio::io::duplex(8192);
    let (client_reader, server_writer) = tokio::io::duplex(8192);

    // Create and configure tenx-mcp server
    let server = MCPServer::default()
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

    // Start tenx-mcp server in background
    let transport = Box::new(transport_helpers::TransportAdapter::new(
        server_reader,
        server_writer,
    ));
    let server_handle = tenx_mcp::MCPServerHandle::new(server, transport)
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

    // Cleanup
    let _ = server_handle.stop().await;
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
                    .ok_or_else(|| rmcp::Error::invalid_params("Missing text parameter", None))?;

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
    let mut client = MCPClient::new();
    let transport = Box::new(transport_helpers::TransportAdapter::new(
        client_reader,
        client_writer,
    ));

    client.connect(transport).await.unwrap();

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
        .call_tool("reverse".to_string(), Some(json!({ "text": "hello" })))
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

// Transport adapter helpers
mod transport_helpers {
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };

    use super::*;

    /// Helper to create a transport that works with tenx-mcp from
    /// AsyncRead/AsyncWrite
    pub struct TransportAdapter<R, W> {
        reader: R,
        writer: W,
        connected: bool,
    }

    impl<R: AsyncRead + Send + Unpin + 'static, W: AsyncWrite + Send + Unpin + 'static>
        TransportAdapter<R, W>
    {
        pub fn new(reader: R, writer: W) -> Self {
            Self {
                reader,
                writer,
                connected: false,
            }
        }
    }

    #[async_trait]
    impl<R, W> tenx_mcp::transport::Transport for TransportAdapter<R, W>
    where
        R: AsyncRead + Send + Sync + Unpin + 'static,
        W: AsyncWrite + Send + Sync + Unpin + 'static,
    {
        async fn connect(&mut self) -> tenx_mcp::error::Result<()> {
            self.connected = true;
            Ok(())
        }

        fn framed(
            self: Box<Self>,
        ) -> tenx_mcp::error::Result<Box<dyn tenx_mcp::transport::TransportStream>> {
            if !self.connected {
                return Err(MCPError::Transport("Not connected".to_string()));
            }

            // Create a duplex stream from reader and writer
            struct DuplexStream<R, W> {
                reader: Pin<Box<R>>,
                writer: Pin<Box<W>>,
            }

            impl<R: AsyncRead, W: AsyncWrite> AsyncRead for DuplexStream<R, W> {
                fn poll_read(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                    buf: &mut tokio::io::ReadBuf<'_>,
                ) -> Poll<std::io::Result<()>> {
                    self.reader.as_mut().poll_read(cx, buf)
                }
            }

            impl<R: AsyncRead, W: AsyncWrite> AsyncWrite for DuplexStream<R, W> {
                fn poll_write(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                    buf: &[u8],
                ) -> Poll<std::io::Result<usize>> {
                    self.writer.as_mut().poll_write(cx, buf)
                }

                fn poll_flush(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                ) -> Poll<std::io::Result<()>> {
                    self.writer.as_mut().poll_flush(cx)
                }

                fn poll_shutdown(
                    mut self: Pin<&mut Self>,
                    cx: &mut Context<'_>,
                ) -> Poll<std::io::Result<()>> {
                    self.writer.as_mut().poll_shutdown(cx)
                }
            }

            let stream = DuplexStream {
                reader: Box::pin(self.reader),
                writer: Box::pin(self.writer),
            };

            let framed =
                tokio_util::codec::Framed::new(stream, tenx_mcp::codec::JsonRpcCodec::new());
            Ok(Box::new(framed))
        }
    }
}
