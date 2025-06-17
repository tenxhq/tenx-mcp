//! Example demonstrating proper error handling in MCP server
//!
//! This example shows how the server properly returns JSON-RPC errors
//! for various error conditions like method not found, invalid params, etc.

use async_trait::async_trait;
use std::collections::HashMap;
use tenx_mcp::{
    connection::Connection,
    error::{MCPError, Result},
    schema::*,
    server::MCPServer,
    transport::{StdioTransport, Transport},
};
use tracing::info;

/// Connection that demonstrates error handling
struct ErrorHandlingConnection {
    server_info: Implementation,
    capabilities: ServerCapabilities,
}

impl ErrorHandlingConnection {
    fn new(server_info: Implementation, capabilities: ServerCapabilities) -> Self {
        Self {
            server_info,
            capabilities,
        }
    }
}

#[async_trait]
impl Connection for ErrorHandlingConnection {
    async fn initialize(
        &mut self,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(
            InitializeResult::new(&self.server_info.name, &self.server_info.version)
                .with_capabilities(self.capabilities.clone()),
        )
    }

    async fn tools_list(&mut self) -> Result<ListToolsResult> {
        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: "always_fail".to_string(),
                    description: Some("A tool that always fails for testing".to_string()),
                    input_schema: ToolInputSchema {
                        schema_type: "object".to_string(),
                        properties: None,
                        required: None,
                    },
                    annotations: None,
                },
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
                },
            ],
            next_cursor: None,
        })
    }

    async fn tools_call(
        &mut self,
        name: String,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        match name.as_str() {
            "always_fail" => Err(MCPError::tool_execution_failed(
                "always_fail",
                "This tool always fails for testing purposes",
            )),
            "strict_tool" => {
                let args = arguments.ok_or_else(|| {
                    MCPError::invalid_params("strict_tool", "Missing arguments object")
                })?;

                let required_field = args
                    .get("required_field")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        MCPError::invalid_params(
                            "strict_tool",
                            "Missing or invalid 'required_field' parameter",
                        )
                    })?;

                Ok(CallToolResult::new()
                    .with_text_content(format!("Received: {required_field}"))
                    .is_error(false))
            }
            _ => Err(MCPError::ToolExecutionFailed {
                tool: name,
                message: "Tool not found".to_string(),
            }),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt().with_target(false).init();

    // Create server with error-prone tools
    let capabilities = ServerCapabilities {
        tools: Some(ToolsCapability {
            list_changed: Some(true),
        }),
        ..Default::default()
    };

    let server_info = Implementation {
        name: "error-example-server".to_string(),
        version: "0.1.0".to_string(),
    };

    let server = MCPServer::default()
        .with_capabilities(capabilities.clone())
        .with_connection_factory(move || {
            Box::new(ErrorHandlingConnection::new(
                server_info.clone(),
                capabilities.clone(),
            ))
        });

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
