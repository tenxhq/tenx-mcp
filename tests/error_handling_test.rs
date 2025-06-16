//! Unit tests for JSON-RPC error handling

use futures::{SinkExt, StreamExt};
use tenx_mcp::{schema::*, server::MCPServer, transport::Transport, MCPServerHandle};

// Reuse test transport from integration test
mod test_helpers {
    use super::*;
    use async_trait::async_trait;
    use tenx_mcp::{codec::JsonRpcCodec, error::Result, transport::TransportStream};
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

use test_helpers::TestTransport;

#[tokio::test]
async fn test_method_not_found() {
    // Create a minimal test connection
    struct TestConnection;

    #[async_trait::async_trait]
    impl tenx_mcp::connection::Connection for TestConnection {
        async fn initialize(
            &mut self,
            _protocol_version: String,
            _capabilities: tenx_mcp::schema::ClientCapabilities,
            _client_info: tenx_mcp::schema::Implementation,
        ) -> tenx_mcp::Result<tenx_mcp::schema::InitializeResult> {
            Ok(InitializeResult {
                protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
                capabilities: ServerCapabilities::default(),
                server_info: Implementation {
                    name: "test-server".to_string(),
                    version: "1.0.0".to_string(),
                },
                instructions: None,
                result: tenx_mcp::schema::Result {
                    meta: None,
                    other: std::collections::HashMap::new(),
                },
            })
        }
    }

    let server = MCPServer::default().with_connection_factory(|| Box::new(TestConnection));
    let (client, server_stream) = tokio::io::duplex(8192);

    let transport: Box<dyn Transport> = Box::new(TestTransport::new(server_stream));
    let server_handle = MCPServerHandle::new(server, transport).await.unwrap();

    let mut transport = Box::new(TestTransport::new(client));
    transport.connect().await.unwrap();
    let mut stream = transport.framed().unwrap();

    stream
        .send(JSONRPCMessage::Request(JSONRPCRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: RequestId::Number(1),
            request: Request {
                method: "non_existent".to_string(),
                params: None,
            },
        }))
        .await
        .unwrap();

    let response = stream.next().await.unwrap().unwrap();
    assert!(matches!(
        response,
        JSONRPCMessage::Error(err) if err.error.code == METHOD_NOT_FOUND
    ));

    drop(stream);
    server_handle.stop().await.unwrap();
}

#[tokio::test]
async fn test_invalid_params() {
    // Create a minimal test connection
    struct TestConnection;

    #[async_trait::async_trait]
    impl tenx_mcp::connection::Connection for TestConnection {
        async fn initialize(
            &mut self,
            _protocol_version: String,
            _capabilities: tenx_mcp::schema::ClientCapabilities,
            _client_info: tenx_mcp::schema::Implementation,
        ) -> tenx_mcp::Result<tenx_mcp::schema::InitializeResult> {
            Ok(InitializeResult {
                protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
                capabilities: ServerCapabilities::default(),
                server_info: Implementation {
                    name: "test-server".to_string(),
                    version: "1.0.0".to_string(),
                },
                instructions: None,
                result: tenx_mcp::schema::Result {
                    meta: None,
                    other: std::collections::HashMap::new(),
                },
            })
        }
    }

    let server = MCPServer::default().with_connection_factory(|| Box::new(TestConnection));
    let (client, server_stream) = tokio::io::duplex(8192);

    let transport: Box<dyn Transport> = Box::new(TestTransport::new(server_stream));
    let server_handle = MCPServerHandle::new(server, transport).await.unwrap();

    let mut transport = Box::new(TestTransport::new(client));
    transport.connect().await.unwrap();
    let mut stream = transport.framed().unwrap();

    stream
        .send(JSONRPCMessage::Request(JSONRPCRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: RequestId::Number(1),
            request: Request {
                method: "initialize".to_string(),
                params: None, // Missing required params
            },
        }))
        .await
        .unwrap();

    let response = stream.next().await.unwrap().unwrap();
    assert!(matches!(
        response,
        JSONRPCMessage::Error(err) if err.error.code == INVALID_PARAMS
    ));

    drop(stream);
    server_handle.stop().await.unwrap();
}

#[tokio::test]
async fn test_successful_response() {
    // Create a minimal test connection
    struct TestConnection;

    #[async_trait::async_trait]
    impl tenx_mcp::connection::Connection for TestConnection {
        async fn initialize(
            &mut self,
            _protocol_version: String,
            _capabilities: tenx_mcp::schema::ClientCapabilities,
            _client_info: tenx_mcp::schema::Implementation,
        ) -> tenx_mcp::Result<tenx_mcp::schema::InitializeResult> {
            Ok(InitializeResult {
                protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
                capabilities: ServerCapabilities {
                    tools: Some(ToolsCapability { list_changed: None }),
                    ..Default::default()
                },
                server_info: Implementation {
                    name: "test-server".to_string(),
                    version: "1.0.0".to_string(),
                },
                instructions: None,
                result: tenx_mcp::schema::Result {
                    meta: None,
                    other: std::collections::HashMap::new(),
                },
            })
        }
    }

    let server = MCPServer::default()
        .with_connection_factory(|| Box::new(TestConnection))
        .with_capabilities(ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: None }),
            ..Default::default()
        });

    let (client_stream, server_stream) = tokio::io::duplex(8192);

    let server_transport = Box::new(TestTransport::new(server_stream));
    let server_handle = MCPServerHandle::new(server, server_transport)
        .await
        .unwrap();

    let mut client_transport = Box::new(TestTransport::new(client_stream));
    client_transport.connect().await.unwrap();
    let mut stream = client_transport.framed().unwrap();

    stream
        .send(JSONRPCMessage::Request(JSONRPCRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: RequestId::String("test-1".to_string()),
            request: Request {
                method: "tools/list".to_string(),
                params: None,
            },
        }))
        .await
        .unwrap();

    let response = stream.next().await.unwrap().unwrap();
    assert!(matches!(response, JSONRPCMessage::Response(_)));

    drop(stream);
    server_handle.stop().await.unwrap();
}
