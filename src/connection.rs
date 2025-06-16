use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::{
    error::Result,
    schema::{
        CallToolResult, CompleteResult, GetPromptResult, InitializeResult, ListPromptsResult,
        ListResourceTemplatesResult, ListResourcesResult, ListRootsResult, ListToolsResult,
        LoggingLevel, ReadResourceResult, ServerNotification,
    },
};

/// Context provided to Connection implementations for interacting with the client
pub struct ConnectionContext {
    /// Sender for server notifications
    pub(crate) notification_tx: broadcast::Sender<ServerNotification>,
}

impl ConnectionContext {
    /// Send a notification to the client
    pub fn send_notification(&self, notification: ServerNotification) -> Result<()> {
        self.notification_tx.send(notification).map_err(|_| {
            crate::error::MCPError::InternalError("Failed to send notification".into())
        })?;
        Ok(())
    }
}

/// Connection trait that server implementers must implement
/// Each client connection will have its own instance of the implementation
#[async_trait]
pub trait Connection: Send + Sync {
    /// Called when a new connection is established
    async fn on_connect(&mut self, _context: ConnectionContext) -> Result<()> {
        Ok(())
    }

    /// Called when the connection is being closed
    async fn on_disconnect(&mut self) -> Result<()> {
        Ok(())
    }

    /// Handle initialize request
    async fn initialize(
        &mut self,
        _protocol_version: String,
        _capabilities: crate::schema::ClientCapabilities,
        _client_info: crate::schema::Implementation,
    ) -> Result<InitializeResult>;

    /// Handle ping request
    async fn ping(&mut self) -> Result<()> {
        Ok(())
    }

    /// List available tools
    async fn tools_list(&mut self) -> Result<ListToolsResult> {
        Ok(ListToolsResult {
            tools: vec![],
            paginated: crate::schema::PaginatedResult {
                next_cursor: None,
                result: crate::schema::Result {
                    meta: None,
                    other: std::collections::HashMap::new(),
                },
            },
        })
    }

    /// Call a tool
    async fn tools_call(
        &mut self,
        name: String,
        _arguments: Option<Value>,
    ) -> Result<CallToolResult> {
        Err(crate::error::MCPError::ToolExecutionFailed {
            tool: name,
            message: "Tool not found".to_string(),
        })
    }

    /// List available resources
    async fn resources_list(&mut self) -> Result<ListResourcesResult> {
        Ok(ListResourcesResult {
            resources: vec![],
            paginated: crate::schema::PaginatedResult {
                next_cursor: None,
                result: crate::schema::Result {
                    meta: None,
                    other: std::collections::HashMap::new(),
                },
            },
        })
    }

    /// List resource templates
    async fn resources_templates_list(&mut self) -> Result<ListResourceTemplatesResult> {
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![],
            paginated: crate::schema::PaginatedResult {
                next_cursor: None,
                result: crate::schema::Result {
                    meta: None,
                    other: std::collections::HashMap::new(),
                },
            },
        })
    }

    /// Read a resource
    async fn resources_read(&mut self, uri: String) -> Result<ReadResourceResult> {
        Err(crate::error::MCPError::ResourceNotFound { uri })
    }

    /// Subscribe to resource updates
    async fn resources_subscribe(&mut self, _uri: String) -> Result<()> {
        Ok(())
    }

    /// Unsubscribe from resource updates
    async fn resources_unsubscribe(&mut self, _uri: String) -> Result<()> {
        Ok(())
    }

    /// List available prompts
    async fn prompts_list(&mut self) -> Result<ListPromptsResult> {
        Ok(ListPromptsResult {
            prompts: vec![],
            paginated: crate::schema::PaginatedResult {
                next_cursor: None,
                result: crate::schema::Result {
                    meta: None,
                    other: std::collections::HashMap::new(),
                },
            },
        })
    }

    /// Get a prompt
    async fn prompts_get(
        &mut self,
        name: String,
        _arguments: Option<std::collections::HashMap<String, serde_json::Value>>,
    ) -> Result<GetPromptResult> {
        Err(crate::error::MCPError::handler_error(
            "prompt",
            format!("Prompt '{name}' not found"),
        ))
    }

    /// Handle completion request
    async fn completion_complete(
        &mut self,
        _reference: crate::schema::Reference,
        _argument: crate::schema::ArgumentInfo,
    ) -> Result<CompleteResult> {
        Ok(CompleteResult {
            completion: crate::schema::CompletionInfo {
                values: vec![],
                total: None,
                has_more: None,
            },
            result: crate::schema::Result {
                meta: None,
                other: std::collections::HashMap::new(),
            },
        })
    }

    /// Set logging level
    async fn logging_set_level(&mut self, _level: LoggingLevel) -> Result<()> {
        Ok(())
    }

    /// List roots (for server-initiated roots/list request)
    async fn roots_list(&mut self) -> Result<ListRootsResult> {
        Ok(ListRootsResult {
            roots: vec![],
            result: crate::schema::Result {
                meta: None,
                other: std::collections::HashMap::new(),
            },
        })
    }

    /// Handle sampling/createMessage request from server
    async fn sampling_create_message(
        &mut self,
        _params: crate::schema::CreateMessageParams,
    ) -> Result<crate::schema::CreateMessageResult> {
        Err(crate::error::MCPError::MethodNotFound(
            "sampling/createMessage".to_string(),
        ))
    }
}
