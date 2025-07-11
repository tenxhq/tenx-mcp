use async_trait::async_trait;

use crate::{
    Error, Result,
    context::{ClientCtx, ServerCtx},
    schema::{
        self, Cursor, ElicitParams, ElicitResult, GetPromptResult, InitializeResult,
        ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, ListRootsResult,
        ListToolsResult, LoggingLevel, ReadResourceResult,
    },
};

/// Connection trait that client implementers must implement.
/// Each client connection will have its own instance of the implementation
///
/// All methods take &self to allow concurrent request handling.
/// Implementations should use interior mutability (Arc<Mutex<_>>, RwLock, etc.)
/// for any mutable state.
#[async_trait]
pub trait ClientConn: Send + Sync + Clone {
    /// Called when a new connection is established
    async fn on_connect(&self, _context: &ClientCtx) -> Result<()> {
        Ok(())
    }

    /// Called when the connection is being closed
    async fn on_shutdown(&self, _context: &ClientCtx) -> Result<()> {
        Ok(())
    }

    /// Respond to a ping request from the server
    async fn pong(&self, _context: &ClientCtx) -> Result<()> {
        Ok(())
    }

    async fn create_message(
        &self,
        _context: &ClientCtx,
        _method: &str,
        _params: schema::CreateMessageParams,
    ) -> Result<schema::CreateMessageResult> {
        Err(Error::InvalidRequest(
            "create_message not implemented".into(),
        ))
    }

    async fn list_roots(&self, _context: &ClientCtx) -> Result<schema::ListRootsResult> {
        Err(Error::InvalidRequest("list_roots not implemented".into()))
    }

    async fn elicit(&self, _context: &ClientCtx, _params: ElicitParams) -> Result<ElicitResult> {
        Err(Error::InvalidRequest("elicit not implemented".into()))
    }

    /// Handle a notification sent from the server
    ///
    /// The default implementation ignores the notification. Implementations
    /// can override this method to react to server-initiated notifications.
    async fn notification(
        &self,
        _context: &ClientCtx,
        _notification: schema::ServerNotification,
    ) -> Result<()> {
        Ok(())
    }
}

/// Connection trait that server implementers must implement
/// Each client connection will have its own instance of the implementation
///
/// All methods take &self to allow concurrent request handling.
/// Implementations should use interior mutability (Arc<Mutex<_>>, RwLock, etc.)
/// for any mutable state.
#[async_trait]
pub trait ServerConn: Send + Sync {
    /// Called when a new connection is established
    ///
    /// # Arguments
    /// * `context` - The server context
    /// * `remote_addr` - The remote address ("stdio" for stdio connections)
    async fn on_connect(&self, _context: &ServerCtx, _remote_addr: &str) -> Result<()> {
        Ok(())
    }

    /// Called when the server is shutting down
    async fn on_shutdown(&self) -> Result<()> {
        Ok(())
    }

    /// Handle initialize request
    async fn initialize(
        &self,
        _context: &ServerCtx,
        _protocol_version: String,
        _capabilities: schema::ClientCapabilities,
        _client_info: schema::Implementation,
    ) -> Result<InitializeResult>;

    /// Respond to a ping request from the client
    async fn pong(&self, _context: &ServerCtx) -> Result<()> {
        Ok(())
    }

    /// List available tools
    async fn list_tools(
        &self,
        _context: &ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListToolsResult> {
        Ok(ListToolsResult::default())
    }

    /// Call a tool
    async fn call_tool(
        &self,
        _context: &ServerCtx,
        name: String,
        _arguments: Option<crate::Arguments>,
    ) -> Result<schema::CallToolResult> {
        Err(Error::ToolExecutionFailed {
            tool: name,
            message: "Tool not found".to_string(),
        })
    }

    /// List available resources
    async fn list_resources(
        &self,
        _context: &ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListResourcesResult> {
        Ok(ListResourcesResult::new())
    }

    /// List resource templates
    async fn list_resource_templates(
        &self,
        _context: &ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListResourceTemplatesResult> {
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![],
            next_cursor: None,
        })
    }

    /// Read a resource
    async fn read_resource(&self, _context: &ServerCtx, uri: String) -> Result<ReadResourceResult> {
        Err(Error::ResourceNotFound { uri })
    }

    /// Subscribe to resource updates
    async fn resources_subscribe(&self, _context: &ServerCtx, _uri: String) -> Result<()> {
        Ok(())
    }

    /// Unsubscribe from resource updates
    async fn resources_unsubscribe(&self, _context: &ServerCtx, _uri: String) -> Result<()> {
        Ok(())
    }

    /// List available prompts
    async fn list_prompts(
        &self,
        _context: &ServerCtx,
        _cursor: Option<Cursor>,
    ) -> Result<ListPromptsResult> {
        Ok(ListPromptsResult::new())
    }

    /// Get a prompt
    async fn get_prompt(
        &self,
        _context: &ServerCtx,
        name: String,
        _arguments: Option<crate::Arguments>,
    ) -> Result<GetPromptResult> {
        Err(Error::handler_error(
            "prompt",
            format!("Prompt '{name}' not found"),
        ))
    }

    /// Handle completion request
    async fn complete(
        &self,
        _context: &ServerCtx,
        _reference: schema::Reference,
        _argument: schema::ArgumentInfo,
    ) -> Result<schema::CompleteResult> {
        Ok(schema::CompleteResult {
            completion: schema::CompletionInfo {
                values: vec![],
                total: None,
                has_more: None,
            },
            _meta: None,
        })
    }

    /// Set logging level
    async fn set_level(&self, _context: &ServerCtx, _level: LoggingLevel) -> Result<()> {
        Ok(())
    }

    /// List roots (for server-initiated roots/list request)
    async fn list_roots(&self, _context: &ServerCtx) -> Result<ListRootsResult> {
        Ok(ListRootsResult {
            roots: vec![],
            _meta: None,
        })
    }

    /// Handle sampling/createMessage request from server
    async fn create_message(
        &self,
        _context: &ServerCtx,
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
        _context: &ServerCtx,
        _notification: schema::ClientNotification,
    ) -> Result<()> {
        Ok(())
    }
}
