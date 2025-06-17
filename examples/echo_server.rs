use std::collections::HashMap;

use async_trait::async_trait;
use tenx_mcp::{
    connection::Connection, error::Error, schema::*, transport::StdioTransport, MCPServer,
    MCPServerHandle, Result,
};
use tracing::{info, level_filters::LevelFilter};

/// Simple echo connection that provides an echo tool
struct EchoConnection {
    server_info: Implementation,
    capabilities: ServerCapabilities,
}

impl EchoConnection {
    fn new(server_info: Implementation, capabilities: ServerCapabilities) -> Self {
        Self {
            server_info,
            capabilities,
        }
    }
}

#[async_trait]
impl Connection for EchoConnection {
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
            tools: vec![Tool {
                name: "echo".to_string(),
                description: Some("Echoes back the input message".to_string()),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some({
                        let mut props = HashMap::new();
                        props.insert(
                            "message".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "The message to echo"
                            }),
                        );
                        props
                    }),
                    required: Some(vec!["message".to_string()]),
                },
                annotations: Some(ToolAnnotations {
                    title: Some("Echo Tool".to_string()),
                    read_only_hint: Some(true),
                    destructive_hint: Some(false),
                    idempotent_hint: Some(true),
                    open_world_hint: Some(false),
                }),
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
            return Err(Error::ToolExecutionFailed {
                tool: name,
                message: "Tool not found".to_string(),
            });
        }

        let message = if let Some(args) = arguments {
            args.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("No message provided")
                .to_string()
        } else {
            "No arguments provided".to_string()
        };

        Ok(CallToolResult::new()
            .with_text_content(format!("Echo: {message}"))
            .is_error(false))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .with_writer(std::io::stderr) // Make sure we do not use stdout to conflict with the protocol
        .init();

    info!("Starting echo MCP server example");

    // Create server
    let capabilities = ServerCapabilities {
        tools: Some(ToolsCapability {
            list_changed: Some(false),
        }),
        resources: None,
        prompts: None,
        logging: None,
        completions: None,
        experimental: None,
    };

    let server_info = Implementation {
        name: "echo-server".to_string(),
        version: "1.0.0".to_string(),
    };

    let server = MCPServer::default()
        .with_capabilities(capabilities.clone())
        .with_connection_factory(move || {
            Box::new(EchoConnection::new(
                server_info.clone(),
                capabilities.clone(),
            ))
        });

    info!("Echo connection configured");

    // Create stdio transport and start serving
    let transport = StdioTransport::new();

    info!("Server ready, starting to serve on stdio");

    let server_handle = MCPServerHandle::new(server, Box::new(transport)).await?;
    server_handle
        .handle
        .await
        .map_err(|e| Error::InternalError(format!("Server task failed: {e}")))?;
    Ok(())
}
