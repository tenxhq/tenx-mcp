use std::{collections::HashMap, sync::Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use async_trait::async_trait;
use futures::{stream::SplitSink, SinkExt, StreamExt};
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, error, info, warn};

use crate::{
    error::{MCPError, Result},
    schema::{self, ToolsCapability, *},
    transport::{Transport, TransportStream},
};

/// Handler trait for MCP server tools
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// Get tool metadata
    fn metadata(&self) -> Tool;

    /// Execute the tool
    async fn execute(&self, arguments: Option<serde_json::Value>) -> Result<Vec<Content>>;
}

/// Handler trait for MCP server resources
#[async_trait]
pub trait ResourceHandler: Send + Sync {
    /// Get resource metadata
    fn metadata(&self) -> Resource;

    /// Read the resource
    async fn read(&self, uri: String) -> Result<Vec<ResourceContents>>;
}

/// Handler trait for MCP server prompts
#[async_trait]
pub trait PromptHandler: Send + Sync {
    /// Get prompt metadata
    fn metadata(&self) -> Prompt;

    /// Get the prompt messages
    async fn get_messages(
        &self,
        arguments: Option<serde_json::Value>,
    ) -> Result<Vec<PromptMessage>>;
}

/// MCP Server implementation
pub struct MCPServer {
    server_info: Implementation,
    capabilities: ServerCapabilities,
    tools: Arc<Mutex<HashMap<String, Box<dyn ToolHandler>>>>,
    resources: Arc<Mutex<HashMap<String, Box<dyn ResourceHandler>>>>,
    prompts: Arc<Mutex<HashMap<String, Box<dyn PromptHandler>>>>,
    notification_tx: broadcast::Sender<ServerNotification>,
    is_running: AtomicBool,
}

impl MCPServer {
    /// Create a new MCP server
    pub fn new(name: String, version: String) -> Self {
        let (notification_tx, _) = broadcast::channel(100);

        Self {
            server_info: Implementation { name, version },
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(true),
                }),
                ..Default::default()
            },
            tools: Arc::new(Mutex::new(HashMap::new())),
            resources: Arc::new(Mutex::new(HashMap::new())),
            prompts: Arc::new(Mutex::new(HashMap::new())),
            notification_tx,
            is_running: AtomicBool::new(false)
        }
    }

    /// Set server capabilities
    pub fn with_capabilities(mut self, capabilities: ServerCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Get server capabilities
    pub fn capabilities(&self) -> &ServerCapabilities {
        &self.capabilities
    }

    /// Register a tool handler
    pub async fn register_tool(&mut self, handler: Box<dyn ToolHandler>) {
        let metadata = handler.metadata();
        let name = metadata.name.clone();
        self.tools.lock().await.insert(name, handler);

        self.send_server_notification(ServerNotification::ToolListChanged);
    }

    /// Register a resource handler
    pub async fn register_resource(&mut self, handler: Box<dyn ResourceHandler>) {
        let metadata = handler.metadata();
        let uri = metadata.uri.clone();
        self.resources.lock().await.insert(uri, handler);
    }

    /// Register a prompt handler
    pub async fn register_prompt(&mut self, handler: Box<dyn PromptHandler>) {
        let metadata = handler.metadata();
        let name = metadata.name.clone();
        self.prompts.lock().await.insert(name, handler);
    }

    /// Remove a tool handler
    pub async fn remove_tool(&mut self, name: &str) -> Option<Box<dyn ToolHandler>> {
        let result = self.tools.lock().await.remove(name);
        if result.is_some() {
            self.send_server_notification(ServerNotification::ToolListChanged);
        }
        result
    }

    /// Send a server notification
    fn send_server_notification(&self, notification: ServerNotification) {
        // TODO Skip sending notifications if specific server capabilities are not enabled
        if self.is_running.load(Ordering::Relaxed) {
            if let Err(e) = self.notification_tx.send(notification.clone()) {
                error!( "Failed to send server notification {:?}: {}", notification, e );
            }
        } else {
            debug!("Skipped sending server notification until server is serving: {:?}", notification)
        }
    }

    /// Start serving connections using the provided transport
    pub async fn serve(&self, mut transport: Box<dyn Transport>) -> Result<()> {
        transport.connect().await?;
        let stream = transport.framed()?;
        let (mut sink_tx, mut stream_rx) = stream.split();
        let mut notification_rx = self.notification_tx.subscribe();

        info!("MCP server started");
        self.is_running.store(true, Ordering::Relaxed);

        loop {
            tokio::select! {
                // Handle incoming messages from client
                result = stream_rx.next() => {
                    match result {
                        Some(Ok(message)) => {
                            if let Err(e) = self.handle_message(message, &mut sink_tx).await {
                                error!("Error handling message: {}", e);
                            }
                        }
                        Some(Err(e)) => {
                            error!("Error reading message: {}", e);
                            break;
                        }
                        None => {
                            info!("Client disconnected");
                            break;
                        }
                    }
                }
                // Forward internal notifications to client
                result = notification_rx.recv() => {
                    match result {
                        Ok(notification) => {
                            let jsonrpc_notification = JSONRPCNotification {
                                jsonrpc: JSONRPC_VERSION.to_string(),
                                notification: Notification {
                                    method: match notification {
                                        ServerNotification::ToolListChanged => "notifications/tools/list_changed",
                                        ServerNotification::ResourceListChanged => "notifications/resources/list_changed",
                                        ServerNotification::PromptListChanged => "notifications/prompts/list_changed",
                                        ServerNotification::ResourceUpdated { .. } => "notifications/resources/updated",
                                        ServerNotification::LoggingMessage { .. } => "notifications/message",
                                        ServerNotification::Progress { .. } => "notifications/progress",
                                        ServerNotification::Cancelled { .. } => "notifications/cancelled",
                                    }.to_string(),
                                    params: match notification {
                                        ServerNotification::ToolListChanged |
                                        ServerNotification::ResourceListChanged |
                                        ServerNotification::PromptListChanged => None,
                                        ServerNotification::ResourceUpdated { uri } => {
                                            let mut params = HashMap::new();
                                            params.insert("uri".to_string(), serde_json::Value::String(uri));
                                            Some(NotificationParams {
                                                meta: None,
                                                other: params,
                                            })
                                        },
                                        ServerNotification::LoggingMessage { level, logger, data } => {
                                            let mut params = HashMap::new();
                                            params.insert("level".to_string(), serde_json::to_value(level).unwrap());
                                            if let Some(logger) = logger {
                                                params.insert("logger".to_string(), serde_json::Value::String(logger));
                                            }
                                            params.insert("data".to_string(), data);
                                            Some(NotificationParams {
                                                meta: None,
                                                other: params,
                                            })
                                        },
                                        ServerNotification::Progress { progress_token, progress, total, message } => {
                                            let mut params = HashMap::new();
                                            params.insert("progressToken".to_string(), serde_json::to_value(progress_token).unwrap());
                                            params.insert("progress".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(progress).unwrap()));
                                            if let Some(total) = total {
                                                params.insert("total".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(total).unwrap()));
                                            }
                                            if let Some(message) = message {
                                                params.insert("message".to_string(), serde_json::Value::String(message));
                                            }
                                            Some(NotificationParams {
                                                meta: None,
                                                other: params,
                                            })
                                        },
                                        ServerNotification::Cancelled { request_id, reason } => {
                                            let mut params = HashMap::new();
                                            params.insert("requestId".to_string(), serde_json::to_value(request_id).unwrap());
                                            if let Some(reason) = reason {
                                                params.insert("reason".to_string(), serde_json::Value::String(reason));
                                            }
                                            Some(NotificationParams {
                                                meta: None,
                                                other: params,
                                            })
                                        },
                                    },
                                },
                            };
                            if let Err(e) = sink_tx.send(JSONRPCMessage::Notification(jsonrpc_notification)).await {
                                error!("Error sending notification to client: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            debug!("Notification channel closed: {}", e);
                            // This is expected when the server shuts down
                        }
                    }
                }
            }
        }

        info!("MCP server stopped");
        Ok(())
    }

    /// Handle an incoming message
    async fn handle_message(
        &self,
        message: JSONRPCMessage,
        sink: &mut SplitSink<Box<dyn TransportStream>, JSONRPCMessage>,
    ) -> Result<()> {
        match message {
            JSONRPCMessage::Request(request) => {
                let response_message = self.handle_request(request.clone()).await;
                sink.send(response_message).await?;
            }
            JSONRPCMessage::Notification(notification) => {
                self.handle_notification(notification).await?;
            }
            JSONRPCMessage::Response(_) => {
                warn!("Server received unexpected response message");
            }
            JSONRPCMessage::Error(error) => {
                warn!("Server received error message: {:?}", error);
            }
            JSONRPCMessage::BatchRequest(_) => {
                // TODO: Implement batch request handling
                error!("Batch requests not yet implemented");
            }
            JSONRPCMessage::BatchResponse(_) => {
                warn!("Server received unexpected batch response");
            }
        }
        Ok(())
    }

    /// Handle a request and generate a response
    async fn handle_request(&self, request: JSONRPCRequest) -> JSONRPCMessage {
        // Convert RequestParams to serde_json::Value
        let params = request
            .request
            .params
            .map(|p| serde_json::to_value(p.other))
            .transpose()
            .map_err(|e| {
                error!("Failed to serialize request params: {}", e);
                e
            })
            .ok()
            .flatten();

        let result_value = match request.request.method.as_str() {
            "initialize" => self.handle_initialize(params).await,
            "tools/list" => self.handle_list_tools().await,
            "tools/call" => self.handle_call_tool(params).await,
            "resources/list" => self.handle_list_resources().await,
            "resources/read" => self.handle_read_resource(params).await,
            "prompts/list" => self.handle_list_prompts().await,
            "prompts/get" => self.handle_get_prompt(params).await,
            _ => Err(MCPError::MethodNotFound(request.request.method.clone())),
        };

        match result_value {
            Ok(value) => {
                // Create a successful response
                JSONRPCMessage::Response(JSONRPCResponse {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: request.id,
                    result: schema::Result {
                        meta: None,
                        other: if let Some(obj) = value.as_object() {
                            obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                        } else {
                            let mut map = HashMap::new();
                            map.insert("result".to_string(), value);
                            map
                        },
                    },
                })
            }
            Err(e) => {
                // Create a proper JSON-RPC error response
                let error_code = match &e {
                    MCPError::MethodNotFound(_) => METHOD_NOT_FOUND,
                    MCPError::InvalidParams { .. } => INVALID_PARAMS,
                    MCPError::InvalidRequest(_) => INVALID_REQUEST,
                    MCPError::Json { .. } | MCPError::InvalidMessageFormat { .. } => PARSE_ERROR,
                    _ => INTERNAL_ERROR,
                };

                JSONRPCMessage::Error(JSONRPCError {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: request.id,
                    error: ErrorObject {
                        code: error_code,
                        message: e.to_string(),
                        data: None,
                    },
                })
            }
        }
    }

    /// Handle a notification
    async fn handle_notification(&self, notification: JSONRPCNotification) -> Result<()> {
        debug!(
            "Received notification: {}",
            notification.notification.method
        );
        // Handle notifications as needed
        Ok(())
    }

    /// Handle initialize request
    async fn handle_initialize(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let _params: InitializeParams = if let Some(p) = params {
            serde_json::from_value(p)?
        } else {
            return Err(MCPError::invalid_params(
                "initialize",
                "Missing required parameters",
            ));
        };

        let result = InitializeResult {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            capabilities: self.capabilities.clone(),
            server_info: self.server_info.clone(),
            instructions: None,
            result: schema::Result {
                meta: None,
                other: HashMap::new(),
            },
        };

        Ok(serde_json::to_value(result)?)
    }

    /// Handle list tools request
    async fn handle_list_tools(&self) -> Result<serde_json::Value> {
        let tools = self.tools.lock().await;
        let tool_list: Vec<Tool> = tools.values().map(|h| h.metadata()).collect();

        let result = ListToolsResult {
            tools: tool_list,
            paginated: PaginatedResult {
                next_cursor: None,
                result: schema::Result {
                    meta: None,
                    other: HashMap::new(),
                },
            },
        };

        Ok(serde_json::to_value(result)?)
    }

    /// Handle call tool request
    async fn handle_call_tool(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let params: CallToolParams = if let Some(p) = params {
            serde_json::from_value(p)?
        } else {
            return Err(MCPError::invalid_params(
                "tools/call",
                "Missing required parameters",
            ));
        };

        let tools = self.tools.lock().await;
        let handler = tools
            .get(&params.name)
            .ok_or_else(|| MCPError::ToolExecutionFailed {
                tool: params.name.clone(),
                message: "Tool not found".to_string(),
            })?;

        let tool_name = params.name.clone();
        let content = handler
            .execute(
                params
                    .arguments
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|e| {
                        MCPError::invalid_params(
                            "tools/call",
                            format!("Invalid arguments format: {e}"),
                        )
                    })?,
            )
            .await
            .map_err(|e| match e {
                MCPError::ToolExecutionFailed { .. } => e,
                MCPError::InvalidParams { .. } => e,
                _ => MCPError::tool_execution_failed(tool_name, e.to_string()),
            })?;

        let result = CallToolResult {
            content,
            is_error: Some(false),
            result: schema::Result {
                meta: None,
                other: HashMap::new(),
            },
        };

        Ok(serde_json::to_value(result)?)
    }

    /// Handle list resources request
    async fn handle_list_resources(&self) -> Result<serde_json::Value> {
        let resources = self.resources.lock().await;
        let resource_list: Vec<Resource> = resources.values().map(|h| h.metadata()).collect();

        let result = ListResourcesResult {
            resources: resource_list,
            paginated: PaginatedResult {
                next_cursor: None,
                result: schema::Result {
                    meta: None,
                    other: HashMap::new(),
                },
            },
        };

        Ok(serde_json::to_value(result)?)
    }

    /// Handle read resource request
    async fn handle_read_resource(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let params: ReadResourceParams = if let Some(p) = params {
            serde_json::from_value(p)?
        } else {
            return Err(MCPError::invalid_params(
                "resources/read",
                "Missing required parameters",
            ));
        };

        let resources = self.resources.lock().await;
        let handler = resources
            .get(&params.uri)
            .ok_or_else(|| MCPError::ResourceNotFound {
                uri: params.uri.clone(),
            })?;

        let uri = params.uri.clone();
        let contents = handler.read(params.uri).await.map_err(|e| match e {
            MCPError::ResourceNotFound { .. } => e,
            _ => MCPError::handler_error("resource", format!("Failed to read '{uri}': {e}")),
        })?;

        let result = ReadResourceResult {
            contents,
            result: schema::Result {
                meta: None,
                other: HashMap::new(),
            },
        };

        Ok(serde_json::to_value(result)?)
    }

    /// Handle list prompts request
    async fn handle_list_prompts(&self) -> Result<serde_json::Value> {
        let prompts = self.prompts.lock().await;
        let prompt_list: Vec<Prompt> = prompts.values().map(|h| h.metadata()).collect();

        let result = ListPromptsResult {
            prompts: prompt_list,
            paginated: PaginatedResult {
                next_cursor: None,
                result: schema::Result {
                    meta: None,
                    other: HashMap::new(),
                },
            },
        };

        Ok(serde_json::to_value(result)?)
    }

    /// Handle get prompt request
    async fn handle_get_prompt(
        &self,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let params: GetPromptParams = if let Some(p) = params {
            serde_json::from_value(p)?
        } else {
            return Err(MCPError::invalid_params(
                "prompts/get",
                "Missing required parameters",
            ));
        };

        let prompts = self.prompts.lock().await;
        let handler = prompts.get(&params.name).ok_or_else(|| {
            MCPError::handler_error("prompt", format!("Prompt '{}' not found", params.name))
        })?;

        let prompt_name = params.name.clone();
        let messages = handler
            .get_messages(
                params
                    .arguments
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(|e| {
                        MCPError::invalid_params(
                            "prompts/get",
                            format!("Invalid arguments format: {e}"),
                        )
                    })?,
            )
            .await
            .map_err(|e| {
                MCPError::handler_error(
                    "prompt",
                    format!("Failed to get prompt '{prompt_name}': {e}"),
                )
            })?;

        let result = GetPromptResult {
            description: None,
            messages,
            result: schema::Result {
                meta: None,
                other: HashMap::new(),
            },
        };

        Ok(serde_json::to_value(result)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    // Mock tool handler for testing
    struct MockToolHandler {
        name: String,
    }

    #[async_trait]
    impl ToolHandler for MockToolHandler {
        fn metadata(&self) -> Tool {
            Tool {
                name: self.name.clone(),
                description: Some("A mock tool for testing".to_string()),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some(HashMap::new()),
                    required: None,
                },
                annotations: None,
            }
        }

        async fn execute(&self, _arguments: Option<serde_json::Value>) -> Result<Vec<Content>> {
            Ok(vec![Content::Text(TextContent {
                text: "Mock result".to_string(),
                annotations: None,
            })])
        }
    }

    #[test]
    fn test_server_creation() {
        let server = MCPServer::new("test-server".to_string(), "1.0.0".to_string());
        assert_eq!(server.server_info.name, "test-server");
        assert_eq!(server.server_info.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_tool_registration_sends_notification() {
        let mut server = MCPServer::new("test-server".to_string(), "1.0.0".to_string());
        let mut notification_rx = server.notification_tx.subscribe();

        // Register a tool
        let tool_handler = Box::new(MockToolHandler {
            name: "test-tool".to_string(),
        });
        server.register_tool(tool_handler).await;

        // Check that we received a notification
        let notification = timeout(Duration::from_millis(100), notification_rx.recv())
            .await
            .expect("Should receive notification within timeout")
            .expect("Should receive notification successfully");

        assert!(matches!(notification, ServerNotification::ToolListChanged));
    }

    #[tokio::test]
    async fn test_tool_removal_sends_notification() {
        let mut server = MCPServer::new("test-server".to_string(), "1.0.0".to_string());
        let mut notification_rx = server.notification_tx.subscribe();

        // First register a tool
        let tool_handler = Box::new(MockToolHandler {
            name: "test-tool".to_string(),
        });
        server.register_tool(tool_handler).await;

        // Drain the first notification
        let _ = notification_rx.recv().await;

        // Remove the tool
        let removed = server.remove_tool("test-tool").await;
        assert!(removed.is_some());

        // Check that we received another notification
        let notification = timeout(Duration::from_millis(100), notification_rx.recv())
            .await
            .expect("Should receive notification within timeout")
            .expect("Should receive notification successfully");

        assert!(matches!(notification, ServerNotification::ToolListChanged));
    }

    #[tokio::test]
    async fn test_remove_nonexistent_tool_no_notification() {
        let mut server = MCPServer::new("test-server".to_string(), "1.0.0".to_string());
        let mut notification_rx = server.notification_tx.subscribe();

        // Try to remove a tool that doesn't exist
        let removed = server.remove_tool("nonexistent-tool").await;
        assert!(removed.is_none());

        // Check that we don't receive any notification
        let result = timeout(Duration::from_millis(100), notification_rx.recv()).await;
        assert!(
            result.is_err(),
            "Should not receive notification for nonexistent tool removal"
        );
    }

    #[tokio::test]
    async fn test_client_receives_tools_list_changed_notification() {
        // This test verifies that when a server registers or removes tools,
        // the notification is properly sent through the notification broadcast system.
        // We test this by simulating the client-server notification flow.

        let mut server = MCPServer::new("test-server".to_string(), "0.1.0".to_string());
        let mut notification_rx = server.notification_tx.subscribe();

        // Register initial tool (this triggers first notification)
        server
            .register_tool(Box::new(MockToolHandler {
                name: "echo".to_string(),
            }))
            .await;

        // Verify first notification
        let notification = timeout(Duration::from_millis(100), notification_rx.recv())
            .await
            .expect("Should receive notification for tool registration")
            .expect("Notification should be received successfully");

        assert!(matches!(notification, ServerNotification::ToolListChanged));

        // Register another tool (this triggers second notification)
        server
            .register_tool(Box::new(MockToolHandler {
                name: "add".to_string(),
            }))
            .await;

        // Verify second notification
        let notification = timeout(Duration::from_millis(100), notification_rx.recv())
            .await
            .expect("Should receive notification for second tool registration")
            .expect("Second notification should be received successfully");

        assert!(matches!(notification, ServerNotification::ToolListChanged));

        // Remove a tool (this triggers third notification)
        let removed = server.remove_tool("echo").await;
        assert!(removed.is_some());

        // Verify third notification
        let notification = timeout(Duration::from_millis(100), notification_rx.recv())
            .await
            .expect("Should receive notification for tool removal")
            .expect("Third notification should be received successfully");

        assert!(matches!(notification, ServerNotification::ToolListChanged));

        // This test demonstrates that:
        // 1. The server broadcasts "notifications/tools/list_changed" when tools are added
        // 2. The server broadcasts "notifications/tools/list_changed" when tools are removed
        // 3. In a real client-server setup, connected clients would receive these notifications
        //    through the server's serve() method which forwards notifications to client connections
    }
}
