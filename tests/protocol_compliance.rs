//! MCP Protocol Compliance Tests
//!
//! This module contains tests that verify our implementation follows the MCP
//! specification correctly. These tests focus on protocol compliance without
//! requiring external dependencies.
//!
//! For actual interoperability tests with rmcp, see rmcp_integration.rs

use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::json;
use tenx_mcp::{
    connection::Connection,
    error::{Error, Result},
    schema::*,
};

/// Test connection implementation with echo and add tools
struct TestConnection {
    tools: HashMap<String, Tool>,
}

impl TestConnection {
    fn new() -> Self {
        let mut tools = HashMap::new();

        // Echo tool
        let echo_schema = ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "message".to_string(),
                    json!({
                        "type": "string",
                        "description": "The message to echo"
                    }),
                );
                props
            }),
            required: Some(vec!["message".to_string()]),
        };

        tools.insert(
            "echo".to_string(),
            Tool::new("echo", echo_schema).with_description("Echoes the input message"),
        );

        // Add tool
        let add_schema = ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some({
                let mut props = HashMap::new();
                props.insert(
                    "a".to_string(),
                    json!({
                        "type": "number",
                        "description": "First number"
                    }),
                );
                props.insert(
                    "b".to_string(),
                    json!({
                        "type": "number",
                        "description": "Second number"
                    }),
                );
                props
            }),
            required: Some(vec!["a".to_string(), "b".to_string()]),
        };

        tools.insert(
            "add".to_string(),
            Tool::new("add", add_schema).with_description("Adds two numbers"),
        );

        Self { tools }
    }
}

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
        let mut result = ListToolsResult::new();
        for tool in self.tools.values() {
            result = result.with_tool(tool.clone());
        }
        Ok(result)
    }

    async fn tools_call(
        &mut self,
        name: String,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        match name.as_str() {
            "echo" => {
                let args = arguments
                    .ok_or_else(|| Error::InvalidParams("echo: Missing arguments".to_string()))?;
                let message = args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        Error::InvalidParams("echo: Missing message parameter".to_string())
                    })?;

                Ok(CallToolResult::new()
                    .with_text_content(message.to_string())
                    .is_error(false))
            }
            "add" => {
                let args = arguments
                    .ok_or_else(|| Error::InvalidParams("add: Missing arguments".to_string()))?;

                let a = args.get("a").and_then(|v| v.as_f64()).ok_or_else(|| {
                    Error::InvalidParams("add: Missing or invalid 'a' parameter".to_string())
                })?;

                let b = args.get("b").and_then(|v| v.as_f64()).ok_or_else(|| {
                    Error::InvalidParams("add: Missing or invalid 'b' parameter".to_string())
                })?;

                Ok(CallToolResult::new()
                    .with_text_content(format!("{}", a + b))
                    .is_error(false))
            }
            _ => Err(Error::ToolExecutionFailed {
                tool: name,
                message: "Tool not found".to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_tool() {
        let mut conn = TestConnection::new();

        // Test tools list
        let tools_result = conn.tools_list().await.unwrap();
        assert_eq!(tools_result.tools.len(), 2);
        let echo_tool = tools_result
            .tools
            .iter()
            .find(|t| t.name == "echo")
            .unwrap();
        assert_eq!(echo_tool.name, "echo");
        assert!(echo_tool.description.is_some());

        // Test execution
        let result = conn
            .tools_call(
                "echo".to_string(),
                Some(json!({ "message": "Hello, World!" })),
            )
            .await
            .unwrap();
        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            Content::Text(text) => assert_eq!(text.text, "Hello, World!"),
            _ => panic!("Expected text content"),
        }

        // Test error on missing arguments
        let error = conn.tools_call("echo".to_string(), None).await.unwrap_err();
        match error {
            Error::InvalidParams(_) => {}
            _ => panic!("Expected InvalidParams error"),
        }

        // Test error on missing message field
        let error = conn
            .tools_call("echo".to_string(), Some(json!({ "wrong_field": "value" })))
            .await
            .unwrap_err();
        match error {
            Error::InvalidParams(_) => {}
            _ => panic!("Expected InvalidParams error"),
        }
    }

    #[tokio::test]
    async fn test_add_tool() {
        let mut conn = TestConnection::new();

        // Test tools list contains add tool
        let tools_result = conn.tools_list().await.unwrap();
        let add_tool = tools_result.tools.iter().find(|t| t.name == "add").unwrap();
        assert_eq!(add_tool.name, "add");
        assert!(add_tool.description.is_some());

        // Test integer addition
        let result = conn
            .tools_call("add".to_string(), Some(json!({ "a": 5, "b": 3 })))
            .await
            .unwrap();
        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            Content::Text(text) => assert_eq!(text.text, "8"),
            _ => panic!("Expected text content"),
        }

        // Test float addition
        let result = conn
            .tools_call("add".to_string(), Some(json!({ "a": 1.5, "b": 2.5 })))
            .await
            .unwrap();
        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            Content::Text(text) => assert_eq!(text.text, "4"),
            _ => panic!("Expected text content"),
        }

        // Test negative numbers
        let result = conn
            .tools_call("add".to_string(), Some(json!({ "a": -5, "b": 3 })))
            .await
            .unwrap();
        assert_eq!(result.content.len(), 1);
        match &result.content[0] {
            Content::Text(text) => assert_eq!(text.text, "-2"),
            _ => panic!("Expected text content"),
        }

        // Test error on missing arguments
        let error = conn.tools_call("add".to_string(), None).await.unwrap_err();
        match error {
            Error::InvalidParams(_) => {}
            _ => panic!("Expected InvalidParams error"),
        }

        // Test error on missing 'a' field
        let error = conn
            .tools_call("add".to_string(), Some(json!({ "b": 5 })))
            .await
            .unwrap_err();
        match error {
            Error::InvalidParams(_) => {}
            _ => panic!("Expected InvalidParams error"),
        }

        // Test error on missing 'b' field
        let error = conn
            .tools_call("add".to_string(), Some(json!({ "a": 5 })))
            .await
            .unwrap_err();
        match error {
            Error::InvalidParams(_) => {}
            _ => panic!("Expected InvalidParams error"),
        }
    }

    #[tokio::test]
    async fn test_protocol_compliance() {
        // Test that our tools follow the MCP protocol specification
        let mut conn = TestConnection::new();
        let tools_result = conn.tools_list().await.unwrap();

        for tool in &tools_result.tools {
            // Verify tool has a name
            assert!(!tool.name.is_empty());

            // Verify input schema is valid
            assert_eq!(tool.input_schema.schema_type, "object");

            // Check it has properties
            assert!(tool.input_schema.properties.is_some());
            let props = tool.input_schema.properties.as_ref().unwrap();
            assert!(!props.is_empty());

            // Check it has required fields
            assert!(tool.input_schema.required.is_some());
            let required = tool.input_schema.required.as_ref().unwrap();
            assert!(!required.is_empty());
        }
    }

    #[test]
    fn test_content_serialization() {
        // Test that Content serializes correctly
        let text_content = Content::Text(TextContent {
            text: "Hello".to_string(),
            annotations: None,
        });

        let json = serde_json::to_value(&text_content).unwrap();
        assert_eq!(json.get("type").and_then(|v| v.as_str()), Some("text"));
        assert_eq!(json.get("text").and_then(|v| v.as_str()), Some("Hello"));
    }
}
