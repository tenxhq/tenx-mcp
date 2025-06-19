use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::{
    schema::{
        self, GetPromptResult, InitializeResult, ListPromptsResult, ListResourceTemplatesResult,
        ListResourcesResult, ListRootsResult, ListToolsResult, LoggingLevel, ReadResourceResult,
    },
    Error, Result,
};

/// Context provided to Connection implementations for interacting with the client
#[derive(Debug, Clone)]
pub struct ServerCtx {
    /// Sender for server notifications
    pub(crate) notification_tx: broadcast::Sender<schema::ClientNotification>,
}

impl ServerCtx {
    /// Create a new ServerConnectionContext with a notification channel
    /// This is primarily intended for testing purposes
    pub fn new(notification_tx: broadcast::Sender<schema::ClientNotification>) -> Self {
        Self { notification_tx }
    }

    /// Send a notification to the client
    pub fn notify(&self, notification: schema::ClientNotification) -> Result<()> {
        self.notification_tx
            .send(notification)
            .map_err(|_| Error::InternalError("Failed to send notification".into()))?;
        Ok(())
    }
}

/// Connection trait that server implementers must implement
/// Each client connection will have its own instance of the implementation
#[async_trait]
pub trait ServerConn: Send + Sync {
    /// Called when a new connection is established
    async fn on_connect(&mut self, _context: ServerCtx) -> Result<()> {
        Ok(())
    }

    /// Called when the connection is being closed
    async fn on_disconnect(&mut self) -> Result<()> {
        Ok(())
    }

    /// Handle initialize request
    async fn initialize(
        &mut self,
        _context: ServerCtx,
        _protocol_version: String,
        _capabilities: schema::ClientCapabilities,
        _client_info: schema::Implementation,
    ) -> Result<InitializeResult>;

    /// Respond to a ping request from the client
    async fn pong(&mut self, _context: ServerCtx) -> Result<()> {
        Ok(())
    }

    /// List available tools
    async fn list_tools(&mut self, _context: ServerCtx) -> Result<ListToolsResult> {
        Ok(ListToolsResult::default())
    }

    /// Call a tool
    async fn tools_call(
        &mut self,
        _context: ServerCtx,
        name: String,
        _arguments: Option<Value>,
    ) -> Result<schema::CallToolResult> {
        Err(Error::ToolExecutionFailed {
            tool: name,
            message: "Tool not found".to_string(),
        })
    }

    /// List available resources
    async fn list_resources(&mut self, _context: ServerCtx) -> Result<ListResourcesResult> {
        Ok(ListResourcesResult {
            resources: vec![],
            next_cursor: None,
        })
    }

    /// List resource templates
    async fn list_resource_templates(
        &mut self,
        _context: ServerCtx,
    ) -> Result<ListResourceTemplatesResult> {
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![],
            next_cursor: None,
        })
    }

    /// Read a resource
    async fn resources_read(
        &mut self,
        _context: ServerCtx,
        uri: String,
    ) -> Result<ReadResourceResult> {
        Err(Error::ResourceNotFound { uri })
    }

    /// Subscribe to resource updates
    async fn resources_subscribe(&mut self, _context: ServerCtx, _uri: String) -> Result<()> {
        Ok(())
    }

    /// Unsubscribe from resource updates
    async fn resources_unsubscribe(&mut self, _context: ServerCtx, _uri: String) -> Result<()> {
        Ok(())
    }

    /// List available prompts
    async fn list_prompts(&mut self, _context: ServerCtx) -> Result<ListPromptsResult> {
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }

    /// Get a prompt
    async fn prompts_get(
        &mut self,
        _context: ServerCtx,
        name: String,
        _arguments: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<GetPromptResult> {
        Err(Error::handler_error(
            "prompt",
            format!("Prompt '{name}' not found"),
        ))
    }

    /// Handle completion request
    async fn completion_complete(
        &mut self,
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
    async fn logging_set_level(&mut self, _context: ServerCtx, _level: LoggingLevel) -> Result<()> {
        Ok(())
    }

    /// List roots (for server-initiated roots/list request)
    async fn list_roots(&mut self, _context: ServerCtx) -> Result<ListRootsResult> {
        Ok(ListRootsResult {
            roots: vec![],
            meta: None,
        })
    }

    /// Handle sampling/createMessage request from server
    async fn sampling_create_message(
        &mut self,
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
        &mut self,
        _context: ServerCtx,
        _notification: schema::ServerNotification,
    ) -> Result<()> {
        Ok(())
    }
}
