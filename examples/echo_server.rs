use std::collections::HashMap;

use async_trait::async_trait;
use tenx_mcp::{MCPServer, Result, ToolHandler, schema::*, transport::StdioTransport};
use tracing::{info, level_filters::LevelFilter};

/// Simple echo tool that returns the input as output
struct EchoTool;

#[async_trait]
impl ToolHandler for EchoTool {
    fn metadata(&self) -> Tool {
        Tool {
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
        }
    }

    async fn execute(&self, arguments: Option<serde_json::Value>) -> Result<Vec<Content>> {
        let message = if let Some(args) = arguments {
            args.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("No message provided")
                .to_string()
        } else {
            "No arguments provided".to_string()
        };

        Ok(vec![Content::Text(TextContent {
            text: format!("Echo: {message}"),
            annotations: None,
        })])
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
    let mut server = MCPServer::new("echo-server".to_string(), "1.0.0".to_string())
        .with_capabilities(ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: Some(false),
            }),
            resources: None,
            prompts: None,
            logging: None,
            completions: None,
            experimental: None,
        });

    // Register the echo tool
    server.register_tool(Box::new(EchoTool)).await;

    info!("Echo tool registered");

    // Create stdio transport and start serving
    let transport = StdioTransport::new();

    info!("Server ready, starting to serve on stdio");

    server.serve(Box::new(transport)).await
}
