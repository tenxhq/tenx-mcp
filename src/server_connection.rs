use std::collections::HashMap;

use async_trait::async_trait;
use serde_json::Value;

use crate::{
    schema::{
        self, Cursor, GetPromptResult, InitializeResult, ListPromptsResult,
        ListResourceTemplatesResult, ListResourcesResult, ListRootsResult, ListToolsResult,
        LoggingLevel, ReadResourceResult,
    },
    server::ServerCtx,
    Error, Result,
};

/// Connection trait that server implementers must implement
/// Each client connection will have its own instance of the implementation
///
/// All methods take &self to allow concurrent request handling.
/// Implementations should use interior mutability (Arc<Mutex<_>>, RwLock, etc.)
/// for any mutable state.
#[async_trait]
pub trait ServerConn: Send + Sync {
    /// Called when a new connection is established
    async fn on_connect(&self, _context: ServerCtx) -> Result<()> {
        Ok(())
    }

    /// Called when the connection is being closed
    async fn on_disconnect(&self) -> Result<()> {
        Ok(())
    }

    /// Handle initialize request
    async fn initialize(
        &self,
        _context: ServerCtx,
        _protocol_version: String,
        _capabilities: schema::ClientCapabilities,
        _client_info: schema::Implementation,
    ) -> Result<InitializeResult>;

    /// Respond to a ping request from the client
    async fn pong(&self, _context: ServerCtx) -> Result<()> {
        Ok(())
    }

    /// List available tools
    async fn tools_list(
        &self,
        _context: ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListToolsResult> {
        Ok(ListToolsResult::default())
    }

    /// Call a tool
    async fn tools_call(
        &self,
        _context: ServerCtx,
        name: String,
        _arguments: Option<HashMap<String, Value>>,
    ) -> Result<schema::CallToolResult> {
        Err(Error::ToolExecutionFailed {
            tool: name,
            message: "Tool not found".to_string(),
        })
    }

    /// List available resources
    async fn list_resources(
        &self,
        _context: ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListResourcesResult> {
        Ok(ListResourcesResult::new())
    }

    /// List resource templates
    async fn list_resource_templates(
        &self,
        _context: ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListResourceTemplatesResult> {
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![],
            next_cursor: None,
        })
    }

    /// Read a resource
    async fn resources_read(&self, _context: ServerCtx, uri: String) -> Result<ReadResourceResult> {
        Err(Error::ResourceNotFound { uri })
    }

    /// Subscribe to resource updates
    async fn resources_subscribe(&self, _context: ServerCtx, _uri: String) -> Result<()> {
        Ok(())
    }

    /// Unsubscribe from resource updates
    async fn resources_unsubscribe(&self, _context: ServerCtx, _uri: String) -> Result<()> {
        Ok(())
    }

    /// List available prompts
    async fn list_prompts(
        &self,
        _context: ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListPromptsResult> {
        Ok(ListPromptsResult::new())
    }

    /// Get a prompt
    async fn prompts_get(
        &self,
        _context: ServerCtx,
        name: String,
        _arguments: Option<std::collections::HashMap<String, String>>,
    ) -> Result<GetPromptResult> {
        Err(Error::handler_error(
            "prompt",
            format!("Prompt '{name}' not found"),
        ))
    }

    /// Handle completion request
    async fn completion_complete(
        &self,
        _context: ServerCtx,
        _reference: schema::Reference,
        _argument: schema::ArgumentInfo,
    ) -> Result<schema::CompleteResult> {
        Ok(schema::CompleteResult {
            completion: schema::CompletionInfo {
                values: vec![],
                total: None,
                has_more: None,
            },
            meta: None,
        })
    }

    /// Set logging level
    async fn logging_set_level(&self, _context: ServerCtx, _level: LoggingLevel) -> Result<()> {
        Ok(())
    }

    /// List roots (for server-initiated roots/list request)
    async fn list_roots(&self, _context: ServerCtx) -> Result<ListRootsResult> {
        Ok(ListRootsResult {
            roots: vec![],
            meta: None,
        })
    }

    /// Handle sampling/createMessage request from server
    async fn sampling_create_message(
        &self,
        _context: ServerCtx,
        _params: schema::CreateMessageParams,
    ) -> Result<schema::CreateMessageResult> {
        Err(Error::MethodNotFound("sampling/createMessage".to_string()))
    }

    /// Handle a notification sent from the client
    ///
    /// The default implementation ignores the notification. Servers can
    /// override this to react to client-initiated notifications such as
    /// progress updates or cancellations.
    async fn notification(
        &self,
        _context: ServerCtx,
        _notification: schema::ClientNotification,
    ) -> Result<()> {
        Ok(())
    }
}
