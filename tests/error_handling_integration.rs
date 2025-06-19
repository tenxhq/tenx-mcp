//! Integration tests verifying server-side JSON-RPC error handling without
//! relying on the JsonRpcCodec. We talk to the server directly over a pair of
//! in-memory duplex streams, manually serialising/deserialising newline
//! delimited JSON.

use async_trait::async_trait;
use std::collections::HashMap;
use tenx_mcp::{
    schema,
    testutils::{make_duplex_pair, read_message, send_message},
    Error, Result, Server, ServerConnection, ServerConnectionContext, ServerHandle,
};
use tokio::io::BufReader;

/// A very small test connection implementation that exposes a single `test`
/// tool which requires a single `required_field` string parameter. Everything
/// else purposefully fails so that we can exercise the server's error
/// responses.
struct TestConnection;

#[async_trait]
impl ServerConnection for TestConnection {
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
                tools: Some(schema::ToolsCapability {
                    list_changed: Some(true),
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
        let schema = schema::ToolInputSchema {
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

        Ok(schema::ListToolsResult::new()
            .with_tool(schema::Tool::new("test", schema).with_description("Test tool")))
    }

    async fn tools_call(
        &mut self,
        _context: ServerConnectionContext,
        name: String,
        arguments: Option<serde_json::Value>,
    ) -> Result<schema::CallToolResult> {
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

        Ok(schema::CallToolResult::new()
            .with_text_content("Success")
            .is_error(false))
    }
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
    let request_unknown = schema::JSONRPCMessage::Request(schema::JSONRPCRequest {
        jsonrpc: schema::JSONRPC_VERSION.to_string(),
        id: schema::RequestId::Number(1),
        request: schema::Request {
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
        schema::JSONRPCMessage::Error(err) => {
            assert_eq!(err.error.code, schema::METHOD_NOT_FOUND);
        }
        other => panic!("unexpected message: {other:?}"),
    }

    // After each read we have to extract the inner reader back so that we can
    // keep using it. `BufReader::into_inner` requires ownership so we take it
    // back between each phase.
    // `reader` continues to own `client_reader`; we simply keep using the same
    // buffered reader for the remainder of the test.

    // --- Test 2: Missing params for initialise --------------------------------
    let request_init_missing = schema::JSONRPCMessage::Request(schema::JSONRPCRequest {
        jsonrpc: schema::JSONRPC_VERSION.to_string(),
        id: schema::RequestId::Number(2),
        request: schema::Request {
            method: "initialize".to_string(),
            params: None,
        },
    });

    send_message(&mut client_writer, &request_init_missing)
        .await
        .expect("send failed");

    let response = read_message(&mut reader).await.expect("read failed");

    match response {
        schema::JSONRPCMessage::Error(err) => {
            assert_eq!(err.error.code, schema::INVALID_PARAMS);
        }
        other => panic!("unexpected message: {other:?}"),
    }

    // Keep using the same reader.

    // --- Test 3: Tool invocation with missing required param ------------------
    let mut params_map = HashMap::new();
    params_map.insert("name".to_string(), serde_json::json!("test"));
    params_map.insert("arguments".to_string(), serde_json::json!({}));

    let request_tool_missing = schema::JSONRPCMessage::Request(schema::JSONRPCRequest {
        jsonrpc: schema::JSONRPC_VERSION.to_string(),
        id: schema::RequestId::Number(3),
        request: schema::Request {
            method: "tools/call".to_string(),
            params: Some(schema::RequestParams {
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
        schema::JSONRPCMessage::Error(err) => {
            assert_eq!(err.error.code, schema::INVALID_PARAMS);
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
