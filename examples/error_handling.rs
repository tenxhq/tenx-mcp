//! Example demonstrating proper error handling in MCP server
//!
//! This example shows how the server properly returns JSON-RPC errors
//! for various error conditions like method not found, invalid params, etc.

use async_trait::async_trait;
use std::collections::HashMap;
use tenx_mcp::{
    error::{MCPError, Result},
    schema::*,
    server::{MCPServer, ToolHandler},
    transport::{StdioTransport, Transport},
};
use tracing::info;

/// A tool that always fails to demonstrate error handling
struct FailingTool;

#[async_trait]
impl ToolHandler for FailingTool {
    fn metadata(&self) -> Tool {
        Tool {
            name: "always_fail".to_string(),
            description: Some("A tool that always fails for testing".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: None,
                required: None,
            },
            annotations: None,
        }
    }

    async fn execute(&self, _arguments: Option<serde_json::Value>) -> Result<Vec<Content>> {
        Err(MCPError::tool_execution_failed(
            "always_fail",
            "This tool always fails for testing purposes",
        ))
    }
}

/// A tool that requires specific parameters
struct StrictTool;

#[async_trait]
impl ToolHandler for StrictTool {
    fn metadata(&self) -> Tool {
        Tool {
            name: "strict_tool".to_string(),
            description: Some("A tool that requires specific parameters".to_string()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some({
                    let mut props = HashMap::new();
                    props.insert(
                        "required_field".to_string(),
                        serde_json::json!({
                            "type": "string",
                            "description": "This field is required"
                        }),
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
            .ok_or_else(|| MCPError::invalid_params("strict_tool", "Missing arguments object"))?;

        let required_field = args
            .get("required_field")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                MCPError::invalid_params(
                    "strict_tool",
                    "Missing or invalid 'required_field' parameter",
                )
            })?;

        Ok(vec![Content::Text(TextContent {
            text: format!("Received: {required_field}"),
            annotations: None,
        })])
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_target(false).init();

    // Create server with error-prone tools
    let mut server = MCPServer::new("error-example-server".to_string(), "0.1.0".to_string())
        .with_capabilities(ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: Some(true),
            }),
            ..Default::default()
        });

    // Register tools
    server.register_tool(Box::new(FailingTool));
    server.register_tool(Box::new(StrictTool));

    info!("Starting MCP server with error handling examples...");
    info!("Try these requests to see error handling:");
    info!(
        "1. Call a non-existent method: {{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"unknown_method\"}}"
    );
    info!(
        "2. Call failing tool: {{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{{\"name\":\"always_fail\"}}}}"
    );
    info!(
        "3. Call strict tool without params: {{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{{\"name\":\"strict_tool\"}}}}"
    );

    // Create and run transport
    let transport: Box<dyn Transport> = Box::new(StdioTransport::new());
    let server_handle = tenx_mcp::MCPServerHandle::new(server, transport).await?;
    server_handle
        .handle
        .await
        .map_err(|e| MCPError::InternalError(format!("Server task failed: {e}")))?;
    Ok(())
}
