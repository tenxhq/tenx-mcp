use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tenx_mcp::{
    macros::*, schema::*, schemars, testutils::TestServerContext, Error, Result, ServerConn,
};

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct EchoParams {
    message: String,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct AddParams {
    a: f64,
    b: f64,
}

#[derive(Debug, Default)]
struct TestServer;

#[mcp_server]
/// Test server with echo and add tools
impl TestServer {
    #[tool]
    /// Echo the message
    async fn echo(
        &self,
        _ctx: &tenx_mcp::ServerCtx,
        params: EchoParams,
    ) -> Result<CallToolResult> {
        Ok(CallToolResult::new().with_text_content(params.message))
    }

    #[tool]
    /// Add two numbers
    async fn add(
        &self,
        _ctx: &tenx_mcp::ServerCtx,
        params: AddParams,
    ) -> Result<CallToolResult> {
        Ok(CallToolResult::new().with_text_content(format!("{}", params.a + params.b)))
    }
}

#[tokio::test]
async fn test_initialize() {
    let server = TestServer;
    let ctx = TestServerContext::new();

    let result = server
        .initialize(
            ctx.ctx(),
            "1.0.0".to_string(),
            ClientCapabilities::default(),
            Implementation {
                name: "test-client".to_string(),
                version: "1.0.0".to_string(),
            },
        )
        .await
        .unwrap();

    assert_eq!(result.server_info.name, "test_server");
    assert_eq!(result.instructions, Some("Test server with echo and add tools".to_string()));
}

#[tokio::test]
async fn test_list_tools() {
    let server = TestServer;
    let ctx = TestServerContext::new();

    let result = server.list_tools(ctx.ctx(), None).await.unwrap();

    assert_eq!(result.tools.len(), 2);
    assert!(result.tools.iter().any(|t| t.name == "echo" && t.description == Some("Echo the message".to_string())));
    assert!(result.tools.iter().any(|t| t.name == "add" && t.description == Some("Add two numbers".to_string())));
}

#[tokio::test]
async fn test_call_tools() {
    let server = TestServer;
    let ctx = TestServerContext::new();

    // Test echo
    let mut args = HashMap::new();
    args.insert("message".to_string(), serde_json::json!("hello"));
    
    let result = server.call_tool(ctx.ctx(), "echo".to_string(), Some(args)).await.unwrap();
    match &result.content[0] {
        Content::Text(text) => assert_eq!(text.text, "hello"),
        _ => panic!("Expected text content"),
    }

    // Test add
    let mut args = HashMap::new();
    args.insert("a".to_string(), serde_json::json!(3.5));
    args.insert("b".to_string(), serde_json::json!(2.5));
    
    let result = server.call_tool(ctx.ctx(), "add".to_string(), Some(args)).await.unwrap();
    match &result.content[0] {
        Content::Text(text) => assert_eq!(text.text, "6"),
        _ => panic!("Expected text content"),
    }
}


#[tokio::test]
async fn test_error_handling() {
    let server = TestServer;
    let ctx = TestServerContext::new();

    // Unknown tool
    let err = server.call_tool(ctx.ctx(), "unknown".to_string(), None).await.unwrap_err();
    assert!(matches!(err, Error::MethodNotFound(_)));

    // Missing arguments
    let err = server.call_tool(ctx.ctx(), "echo".to_string(), None).await.unwrap_err();
    assert!(matches!(err, Error::InvalidParams(_)));

    // Invalid arguments
    let mut args = HashMap::new();
    args.insert("a".to_string(), serde_json::json!("not a number"));
    args.insert("b".to_string(), serde_json::json!(2.0));
    
    let err = server.call_tool(ctx.ctx(), "add".to_string(), Some(args)).await.unwrap_err();
    assert!(matches!(err, Error::InvalidParams(_)));
}

