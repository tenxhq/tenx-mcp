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
    error::MCPError,
    schema::{Content, TextContent, Tool, ToolInputSchema},
    MCPServer, ToolHandler,
};

/// Test implementation of an echo tool
struct EchoTool;

#[async_trait]
impl ToolHandler for EchoTool {
    fn metadata(&self) -> Tool {
        Tool {
            name: "echo".to_string(),
            description: Some("Echoes the input message".to_string()),
            input_schema: ToolInputSchema {
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
            },
            annotations: None,
        }
    }

    async fn execute(
        &self,
        arguments: Option<serde_json::Value>,
    ) -> Result<Vec<Content>, MCPError> {
        let args =
            arguments.ok_or_else(|| MCPError::invalid_params("echo", "Missing arguments"))?;
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| MCPError::invalid_params("echo", "Missing message parameter"))?;

        Ok(vec![Content::Text(TextContent {
            text: message.to_string(),
            annotations: None,
        })])
    }
}

/// Test implementation of an add tool
struct AddTool;

#[async_trait]
impl ToolHandler for AddTool {
    fn metadata(&self) -> Tool {
        Tool {
            name: "add".to_string(),
            description: Some("Adds two numbers".to_string()),
            input_schema: ToolInputSchema {
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
            },
            annotations: None,
        }
    }

    async fn execute(
        &self,
        arguments: Option<serde_json::Value>,
    ) -> Result<Vec<Content>, MCPError> {
        let args = arguments.ok_or_else(|| MCPError::invalid_params("add", "Missing arguments"))?;

        let a = args
            .get("a")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| MCPError::invalid_params("add", "Missing or invalid 'a' parameter"))?;

        let b = args
            .get("b")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| MCPError::invalid_params("add", "Missing or invalid 'b' parameter"))?;

        Ok(vec![Content::Text(TextContent {
            text: format!("{}", a + b),
            annotations: None,
        })])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_tool() {
        let tool = EchoTool;

        // Test metadata
        let metadata = tool.metadata();
        assert_eq!(metadata.name, "echo");
        assert!(metadata.description.is_some());

        // Test execution
        let result = tool
            .execute(Some(json!({ "message": "Hello, World!" })))
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Content::Text(text) => assert_eq!(text.text, "Hello, World!"),
            _ => panic!("Expected text content"),
        }

        // Test error on missing arguments
        let error = tool.execute(None).await.unwrap_err();
        match error {
            MCPError::InvalidParams { .. } => {}
            _ => panic!("Expected InvalidParams error"),
        }

        // Test error on missing message field
        let error = tool
            .execute(Some(json!({ "wrong_field": "value" })))
            .await
            .unwrap_err();
        match error {
            MCPError::InvalidParams { .. } => {}
            _ => panic!("Expected InvalidParams error"),
        }
    }

    #[tokio::test]
    async fn test_add_tool() {
        let tool = AddTool;

        // Test metadata
        let metadata = tool.metadata();
        assert_eq!(metadata.name, "add");
        assert!(metadata.description.is_some());

        // Test integer addition
        let result = tool.execute(Some(json!({ "a": 5, "b": 3 }))).await.unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Content::Text(text) => assert_eq!(text.text, "8"),
            _ => panic!("Expected text content"),
        }

        // Test float addition
        let result = tool
            .execute(Some(json!({ "a": 1.5, "b": 2.5 })))
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Content::Text(text) => assert_eq!(text.text, "4"),
            _ => panic!("Expected text content"),
        }

        // Test negative numbers
        let result = tool
            .execute(Some(json!({ "a": -5, "b": 3 })))
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        match &result[0] {
            Content::Text(text) => assert_eq!(text.text, "-2"),
            _ => panic!("Expected text content"),
        }

        // Test error on missing arguments
        let error = tool.execute(None).await.unwrap_err();
        match error {
            MCPError::InvalidParams { .. } => {}
            _ => panic!("Expected InvalidParams error"),
        }

        // Test error on missing 'a' field
        let error = tool.execute(Some(json!({ "b": 5 }))).await.unwrap_err();
        match error {
            MCPError::InvalidParams { .. } => {}
            _ => panic!("Expected InvalidParams error"),
        }

        // Test error on missing 'b' field
        let error = tool.execute(Some(json!({ "a": 5 }))).await.unwrap_err();
        match error {
            MCPError::InvalidParams { .. } => {}
            _ => panic!("Expected InvalidParams error"),
        }
    }

    #[tokio::test]
    async fn test_protocol_compliance() {
        // Test that our tools follow the MCP protocol specification
        let tools: Vec<Box<dyn ToolHandler>> = vec![Box::new(EchoTool), Box::new(AddTool)];

        for tool in tools {
            let metadata = tool.metadata();

            // Verify tool has a name
            assert!(!metadata.name.is_empty());

            // Verify input schema is valid
            assert_eq!(metadata.input_schema.schema_type, "object");

            // Check it has properties
            assert!(metadata.input_schema.properties.is_some());
            let props = metadata.input_schema.properties.as_ref().unwrap();
            assert!(!props.is_empty());

            // Check it has required fields
            assert!(metadata.input_schema.required.is_some());
            let required = metadata.input_schema.required.as_ref().unwrap();
            assert!(!required.is_empty());
        }
    }

    #[tokio::test]
    async fn test_server_initialization() {
        // Test server creation with tools
        let mut server = MCPServer::new("test-server".to_string(), "0.1.0".to_string());

        // Register tools
        server.register_tool(Box::new(EchoTool));
        server.register_tool(Box::new(AddTool));

        // Verify that tools capability is set by default
        let capabilities = server.capabilities();
        assert!(capabilities.tools.is_some());
        assert_eq!(
            capabilities.tools.as_ref().unwrap().list_changed,
            Some(true)
        );

        // Server is ready to handle requests
        // In a real test, we would connect a client and test the interaction
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

    #[tokio::test]
    async fn test_server_broadcasts_tools_list_changed_notification() {
        // Create a server
        let mut server = MCPServer::new("test-server".to_string(), "0.1.0".to_string());

        // The test verifies that:
        // 1. Server has tools capability with list_changed: true
        let capabilities = server.capabilities();
        assert!(capabilities.tools.is_some());
        assert_eq!(
            capabilities.tools.as_ref().unwrap().list_changed,
            Some(true)
        );

        // 2. Register a new tool (this should trigger internal notification)
        server.register_tool(Box::new(EchoTool));
        server.register_tool(Box::new(AddTool));

        // 3. Remove a tool (this should also trigger internal notification)
        let removed_tool = server.remove_tool("echo").await;
        assert!(removed_tool.is_some());

        // TODO Connect an MCPClient and make sure it gets the actual notification
    }
}
