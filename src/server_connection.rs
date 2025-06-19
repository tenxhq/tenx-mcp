use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::{
    error::Result,
    schema::{
        self, CallToolResult, CompleteResult, GetPromptResult, InitializeResult, ListPromptsResult,
        ListResourceTemplatesResult, ListResourcesResult, ListRootsResult, ListToolsResult,
        LoggingLevel, ReadResourceResult,
    },
};

/// Factory function type for creating Connection instances
pub type ServerConnectionFactory = Box<dyn Fn() -> Box<dyn ServerConnection> + Send + Sync>;

/// Context provided to Connection implementations for interacting with the client
#[derive(Debug, Clone)]
pub struct ServerConnectionContext {
    /// Sender for server notifications
    pub(crate) notification_tx: broadcast::Sender<schema::ClientNotification>,
}

impl ServerConnectionContext {
    /// Create a new ServerConnectionContext with a notification channel
    /// This is primarily intended for testing purposes
    pub fn new(notification_tx: broadcast::Sender<schema::ClientNotification>) -> Self {
        Self { notification_tx }
    }

    /// Send a notification to the client
    pub fn send_notification(&self, notification: schema::ClientNotification) -> Result<()> {
        self.notification_tx.send(notification).map_err(|_| {
            crate::error::Error::InternalError("Failed to send notification".into())
        })?;
        Ok(())
    }
}

/// Connection trait that server implementers must implement
/// Each client connection will have its own instance of the implementation
#[async_trait]
pub trait ServerConnection: Send + Sync {
    /// Called when a new connection is established
    async fn on_connect(&mut self, _context: ServerConnectionContext) -> Result<()> {
        Ok(())
    }

    /// Called when the connection is being closed
    async fn on_disconnect(&mut self) -> Result<()> {
        Ok(())
    }

    /// Handle initialize request
    async fn initialize(
        &mut self,
        _context: ServerConnectionContext,
        _protocol_version: String,
        _capabilities: crate::schema::ClientCapabilities,
        _client_info: crate::schema::Implementation,
    ) -> Result<InitializeResult>;

    /// Respond to a ping request
    async fn pong(&mut self, _context: ServerConnectionContext) -> Result<()> {
        Ok(())
    }

    /// List available tools
    async fn tools_list(&mut self, _context: ServerConnectionContext) -> Result<ListToolsResult> {
        Ok(ListToolsResult::default())
    }

    /// Call a tool
    async fn tools_call(
        &mut self,
        _context: ServerConnectionContext,
        name: String,
        _arguments: Option<Value>,
    ) -> Result<CallToolResult> {
        Err(crate::error::Error::ToolExecutionFailed {
            tool: name,
            message: "Tool not found".to_string(),
        })
    }

    /// List available resources
    async fn resources_list(
        &mut self,
        _context: ServerConnectionContext,
    ) -> Result<ListResourcesResult> {
        Ok(ListResourcesResult {
            resources: vec![],
            next_cursor: None,
        })
    }

    /// List resource templates
    async fn resources_templates_list(
        &mut self,
        _context: ServerConnectionContext,
    ) -> Result<ListResourceTemplatesResult> {
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![],
            next_cursor: None,
        })
    }

    /// Read a resource
    async fn resources_read(
        &mut self,
        _context: ServerConnectionContext,
        uri: String,
    ) -> Result<ReadResourceResult> {
        Err(crate::error::Error::ResourceNotFound { uri })
    }

    /// Subscribe to resource updates
    async fn resources_subscribe(
        &mut self,
        _context: ServerConnectionContext,
        _uri: String,
    ) -> Result<()> {
        Ok(())
    }

    /// Unsubscribe from resource updates
    async fn resources_unsubscribe(
        &mut self,
        _context: ServerConnectionContext,
        _uri: String,
    ) -> Result<()> {
        Ok(())
    }

    /// List available prompts
    async fn prompts_list(
        &mut self,
        _context: ServerConnectionContext,
    ) -> Result<ListPromptsResult> {
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }

    /// Get a prompt
    async fn prompts_get(
        &mut self,
        _context: ServerConnectionContext,
        name: String,
        _arguments: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<GetPromptResult> {
        Err(crate::error::Error::handler_error(
            "prompt",
            format!("Prompt '{name}' not found"),
        ))
    }

    /// Handle completion request
    async fn completion_complete(
        &mut self,
        _context: ServerConnectionContext,
        _reference: crate::schema::Reference,
        _argument: crate::schema::ArgumentInfo,
    ) -> Result<CompleteResult> {
        Ok(CompleteResult {
            completion: crate::schema::CompletionInfo {
                values: vec![],
                total: None,
                has_more: None,
            },
            meta: None,
        })
    }

    /// Set logging level
    async fn logging_set_level(
        &mut self,
        _context: ServerConnectionContext,
        _level: LoggingLevel,
    ) -> Result<()> {
        Ok(())
    }

    /// List roots (for server-initiated roots/list request)
    async fn roots_list(&mut self, _context: ServerConnectionContext) -> Result<ListRootsResult> {
        Ok(ListRootsResult {
            roots: vec![],
            meta: None,
        })
    }

    /// Handle sampling/createMessage request from server
    async fn sampling_create_message(
        &mut self,
        _context: ServerConnectionContext,
        _params: crate::schema::CreateMessageParams,
    ) -> Result<crate::schema::CreateMessageResult> {
        Err(crate::error::Error::MethodNotFound(
            "sampling/createMessage".to_string(),
        ))
    }

    /// Handle a notification sent from the client
    ///
    /// The default implementation ignores the notification. Servers can
    /// override this to react to client-initiated notifications such as
    /// progress updates or cancellations.
    async fn notification(
        &mut self,
        _context: ServerConnectionContext,
        _notification: schema::ServerNotification,
    ) -> Result<()> {
        Ok(())
    }
}
