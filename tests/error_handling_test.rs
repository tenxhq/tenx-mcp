use futures::{SinkExt, StreamExt};
use tenx_mcp::{
    codec::JsonRpcCodec, connection::Connection, schema::*, server::Server, Result, ServerHandle,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::Framed;

// Minimal test connection for error testing
struct TestConnection;

#[async_trait::async_trait]
impl Connection for TestConnection {
    async fn initialize(
        &mut self,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities::default(),
            server_info: Implementation {
                name: "test-server".to_string(),
                version: "1.0.0".to_string(),
            },
            instructions: None,
            meta: None,
        })
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
async fn test_method_not_found() {
    let server = Server::default().with_connection_factory(|| Box::new(TestConnection));

    let (client_stream, server_reader, server_writer) = create_test_connection();

    // Start server with the proper read/write streams
    let server_handle = ServerHandle::from_stream(server, server_reader, server_writer)
        .await
        .unwrap();

    // Create client using the combined stream
    let mut client = Framed::new(client_stream, JsonRpcCodec::new());

    // Send request to non-existent method
    client
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

    // Verify we get a method not found error
    let response = client.next().await.unwrap().unwrap();
    assert!(matches!(
        response,
        JSONRPCMessage::Error(err) if err.error.code == METHOD_NOT_FOUND
    ));

    drop(client);
    server_handle.stop().await.unwrap();
}

#[tokio::test]
async fn test_invalid_params() {
    let server = Server::default().with_connection_factory(|| Box::new(TestConnection));

    let (client_stream, server_reader, server_writer) = create_test_connection();

    // Start server with the proper read/write streams
    let server_handle = ServerHandle::from_stream(server, server_reader, server_writer)
        .await
        .unwrap();

    // Create client using the combined stream
    let mut client = Framed::new(client_stream, JsonRpcCodec::new());

    // Send initialize request without required params
    client
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

    // Verify we get an invalid params error
    let response = client.next().await.unwrap().unwrap();
    assert!(matches!(
        response,
        JSONRPCMessage::Error(err) if err.error.code == INVALID_PARAMS
    ));

    drop(client);
    server_handle.stop().await.unwrap();
}

#[tokio::test]
async fn test_successful_response() {
    // Create server with tools capability
    let server = Server::default()
        .with_connection_factory(|| Box::new(TestConnection))
        .with_capabilities(ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: None }),
            ..Default::default()
        });

    let (client_stream, server_reader, server_writer) = create_test_connection();

    // Start server with the proper read/write streams
    let server_handle = ServerHandle::from_stream(server, server_reader, server_writer)
        .await
        .unwrap();

    // Create client using the combined stream
    let mut client = Framed::new(client_stream, JsonRpcCodec::new());

    // Send valid tools/list request
    client
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

    // Verify we get a successful response
    let response = client.next().await.unwrap().unwrap();
    assert!(matches!(response, JSONRPCMessage::Response(_)));

    drop(client);
    server_handle.stop().await.unwrap();
}
