use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use tenx_mcp::{
    Client, Result, Server, ServerAPI, ServerConn, ServerCtx,
    schema::{self, *},
};

#[derive(Default)]
struct EchoConnection;

#[async_trait]
impl ServerConn for EchoConnection {
    async fn initialize(
        &self,
        _context: &ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new("http-echo-server")
            .with_version("0.1.0")
            .with_capabilities(ServerCapabilities::default().with_tools(None)))
    }

    async fn list_tools(
        &self,
        _context: &ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListToolsResult> {
        let schema = ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert("message".to_string(), json!({"type": "string"}));
                props
            }),
            required: Some(vec!["message".to_string()]),
        };
        Ok(ListToolsResult::default()
            .with_tool(Tool::new("echo", schema).with_description("Echo message")))
    }

    async fn call_tool(
        &self,
        _context: &ServerCtx,
        name: String,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> Result<CallToolResult> {
        use tenx_mcp::Error;
        if name != "echo" {
            return Err(Error::ToolNotFound(name));
        }
        let args = arguments.ok_or_else(|| Error::InvalidParams("Missing args".into()))?;
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidParams("Missing message".into()))?;
        Ok(CallToolResult::new()
            .with_text_content(message.to_string())
            .is_error(false))
    }
}

#[tokio::test]
async fn test_http_echo_tool_integration() {
    let _ = tracing_subscriber::fmt::try_init();

    // Use port 0 to let the OS assign an available port
    let server = Server::default().with_connection(EchoConnection::default);
    let server_handle = server.serve_http("127.0.0.1:0").await.unwrap();

    // Get the actual bound address
    let bound_addr = server_handle.bound_addr.as_ref().unwrap();
    // Small delay to ensure server is fully ready
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Connect HTTP client
    let mut client = Client::new("http-test-client", "0.1.0");
    let init = client
        .connect_http(&format!("http://{bound_addr}"))
        .await
        .unwrap();
    assert_eq!(init.server_info.name, "http-echo-server");

    // Call echo tool
    let mut args = HashMap::new();
    args.insert("message".to_string(), json!("hello"));
    let result = client.call_tool("echo", args).await.unwrap();
    if let Some(schema::Content::Text(text)) = result.content.first() {
        assert_eq!(text.text, "hello");
    } else {
        panic!("expected text response");
    }

    drop(client);
    // Properly stop the server
    server_handle.stop().await.unwrap();
}
