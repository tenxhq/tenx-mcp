//! Integration tests verifying server-side JSON-RPC error handling without
//! relying on the JsonRpcCodec. We talk to the server directly over a pair of
//! in-memory duplex streams, manually serialising/deserialising newline
//! delimited JSON.

use async_trait::async_trait;
use std::collections::HashMap;
use tenx_mcp::{
    connection::Connection,
    error::{Error, Result},
    schema::*,
    server::Server,
    ServerHandle,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

/// A very small test connection implementation that exposes a single `test`
/// tool which requires a single `required_field` string parameter. Everything
/// else purposefully fails so that we can exercise the server's error
/// responses.
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
        // We only handle the `test` tool in this fake implementation.
        if name != "test" {
            return Err(Error::ToolExecutionFailed {
                tool: name,
                message: "Tool not found".to_string(),
            });
        }

        let args = arguments
            .ok_or_else(|| Error::InvalidParams("strict_params: Missing arguments".into()))?;

        let _ = args
            .get("required_field")
            .ok_or_else(|| Error::InvalidParams("strict_params: Missing required_field".into()))?;

        Ok(CallToolResult::new()
            .with_text_content("Success")
            .is_error(false))
    }
}

/// Convenience helper that creates two independent in-memory duplex pipes that
/// together form a bidirectional channel. The returned tuple is laid out so
/// that the first element can be given to the server as its reader, the second
/// as the server writer, and the remaining two are the reader/writer pair for
/// the client side of the connection.
fn make_duplex_pair() -> (
    impl AsyncRead + Send + Sync + Unpin + 'static,
    impl AsyncWrite + Send + Sync + Unpin + 'static,
    impl AsyncRead + Send + Sync + Unpin + 'static,
    impl AsyncWrite + Send + Sync + Unpin + 'static,
) {
    let (server_reader, client_writer) = tokio::io::duplex(8 * 1024);
    let (client_reader, server_writer) = tokio::io::duplex(8 * 1024);
    (server_reader, server_writer, client_reader, client_writer)
}

/// Serialise a `JSONRPCMessage`, append a `\n` delimiter and send it over the
/// provided writer.
async fn send_message<W>(writer: &mut W, message: &JSONRPCMessage) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let json = serde_json::to_vec(message)?;
    writer.write_all(&json).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

/// Read a single newline-delimited JSON-RPC message from the reader.
async fn read_message<R>(reader: &mut BufReader<R>) -> Result<JSONRPCMessage>
where
    R: AsyncRead + Unpin,
{
    let mut buf = Vec::new();
    reader.read_until(b'\n', &mut buf).await?;
    if buf.is_empty() {
        return Err(Error::Transport("Stream closed".into()));
    }
    // Remove the trailing '\n'.
    if buf.last() == Some(&b'\n') {
        buf.pop();
    }
    Ok(serde_json::from_slice(&buf)?)
}

#[tokio::test]
async fn test_error_responses() {
    // Install tracing so that if the test hangs we at least get some idea of
    // what happened.
    let _ = tracing_subscriber::fmt::try_init();

    // Spin up our test server backed by the `TestConnection` implementation.
    let server = Server::default().with_connection_factory(|| Box::new(TestConnection));

    let (server_reader, server_writer, client_reader, mut client_writer) = make_duplex_pair();

    // Launch the server – this will run until either side of the transport is
    // closed.
    let server_handle: ServerHandle =
        ServerHandle::from_stream(server, server_reader, server_writer)
            .await
            .expect("failed to start server");

    // --- Test 1: Unknown method ------------------------------------------------
    let request_unknown = JSONRPCMessage::Request(JSONRPCRequest {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: RequestId::Number(1),
        request: Request {
            method: "unknown".to_string(),
            params: None,
        },
    });

    let mut reader = BufReader::new(client_reader);

    send_message(&mut client_writer, &request_unknown)
        .await
        .expect("send failed");

    let response = read_message(&mut reader).await.expect("read failed");

    match response {
        JSONRPCMessage::Error(err) => {
            assert_eq!(err.error.code, METHOD_NOT_FOUND);
        }
        other => panic!("unexpected message: {other:?}"),
    }

    // After each read we have to extract the inner reader back so that we can
    // keep using it. `BufReader::into_inner` requires ownership so we take it
    // back between each phase.
    // `reader` continues to own `client_reader`; we simply keep using the same
    // buffered reader for the remainder of the test.

    // --- Test 2: Missing params for initialise --------------------------------
    let request_init_missing = JSONRPCMessage::Request(JSONRPCRequest {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: RequestId::Number(2),
        request: Request {
            method: "initialize".to_string(),
            params: None,
        },
    });

    send_message(&mut client_writer, &request_init_missing)
        .await
        .expect("send failed");

    let response = read_message(&mut reader).await.expect("read failed");

    match response {
        JSONRPCMessage::Error(err) => {
            assert_eq!(err.error.code, INVALID_PARAMS);
        }
        other => panic!("unexpected message: {other:?}"),
    }

    // Keep using the same reader.

    // --- Test 3: Tool invocation with missing required param ------------------
    let mut params_map = HashMap::new();
    params_map.insert("name".to_string(), serde_json::json!("test"));
    params_map.insert("arguments".to_string(), serde_json::json!({}));

    let request_tool_missing = JSONRPCMessage::Request(JSONRPCRequest {
        jsonrpc: JSONRPC_VERSION.to_string(),
        id: RequestId::Number(3),
        request: Request {
            method: "tools/call".to_string(),
            params: Some(RequestParams {
                meta: None,
                other: params_map,
            }),
        },
    });

    send_message(&mut client_writer, &request_tool_missing)
        .await
        .expect("send failed");

    let response = read_message(&mut reader).await.expect("read failed");

    match response {
        JSONRPCMessage::Error(err) => {
            assert_eq!(err.error.code, INVALID_PARAMS);
            assert!(err.error.message.contains("Missing required_field"));
        }
        other => panic!("unexpected message: {other:?}"),
    }

    // -------------------------------------------------------------------------
    // Clean-up. Drop the client side of the connection and then ask the server
    // to shut down. We wrap the shutdown in a short timeout so that the test
    // doesn't hang in the unlikely event of a bug.
    drop(client_writer);
    drop(reader);

    let _ = tokio::time::timeout(std::time::Duration::from_millis(100), server_handle.stop()).await;
}
