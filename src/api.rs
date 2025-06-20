use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

use crate::error::Result;
use crate::schema::{
    ArgumentInfo, CallToolResult, ClientCapabilities, CompleteResult, CreateMessageParams,
    CreateMessageResult, Cursor, GetPromptResult, Implementation, InitializeResult,
    ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, ListRootsResult,
    ListToolsResult, LoggingLevel, ReadResourceResult, Reference,
};

/// Server API trait defining all methods that an MCP server must implement.
/// These methods are called by the client.
#[async_trait]
pub trait ServerAPI: Send + Sync {
    /// Initialize the connection with protocol version and capabilities
    async fn initialize(
        &self,
        protocol_version: String,
        capabilities: ClientCapabilities,
        client_info: Implementation,
    ) -> Result<InitializeResult>;

    /// Respond to ping requests
    async fn ping(&self) -> Result<()>;

    /// List available tools with optional pagination
    async fn list_tools(&self, cursor: Option<Cursor>) -> Result<ListToolsResult>;

    /// Call a tool with the given name and arguments
    async fn tools_call(
        &self,
        name: impl Into<String>,
        arguments: Option<HashMap<String, Value>>,
    ) -> Result<CallToolResult>;

    /// List available resources with optional pagination
    async fn list_resources(&self, cursor: Option<Cursor>) -> Result<ListResourcesResult>;

    /// List resource templates with optional pagination
    async fn list_resource_templates(
        &self,
        cursor: Option<Cursor>,
    ) -> Result<ListResourceTemplatesResult>;

    /// Read a resource by URI
    async fn resources_read(&self, uri: impl Into<String>) -> Result<ReadResourceResult>;

    /// Subscribe to resource updates
    async fn resources_subscribe(&self, uri: impl Into<String>) -> Result<()>;

    /// Unsubscribe from resource updates
    async fn resources_unsubscribe(&self, uri: impl Into<String>) -> Result<()>;

    /// List available prompts with optional pagination
    async fn list_prompts(&self, cursor: Option<Cursor>) -> Result<ListPromptsResult>;

    /// Get a prompt by name with optional arguments
    async fn get_prompt(
        &self,
        name: impl Into<String>,
        arguments: Option<HashMap<String, String>>,
    ) -> Result<GetPromptResult>;

    /// Handle completion requests
    async fn complete(
        &self,
        reference: Reference,
        argument: ArgumentInfo,
    ) -> Result<CompleteResult>;

    /// Set the logging level
    async fn set_level(&self, level: LoggingLevel) -> Result<()>;
}

/// Client API trait defining all methods that an MCP client must implement.
/// These methods are called by the server.
#[async_trait]
pub trait ClientAPI: Send + Sync {
    /// Respond to ping requests from the server
    async fn ping(&self) -> Result<()>;

    /// Handle LLM sampling requests from the server
    async fn create_message(&self, params: CreateMessageParams) -> Result<CreateMessageResult>;

    /// List available filesystem roots
    async fn list_roots(&self) -> Result<ListRootsResult>;
}

