use async_trait::async_trait;
use futures::{stream::SplitSink, SinkExt, StreamExt};
use std::collections::HashMap;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
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

/// Commands sent to the server for runtime registration
enum ServerCommand {
    RegisterTool(Box<dyn ToolHandler>),
    RemoveTool(String),
}
type CommandTuple = (ServerCommand, tokio::sync::oneshot::Sender<Result<()>>);

/// MCP Server implementation
pub struct MCPServer {
    server_info: Implementation,
    capabilities: ServerCapabilities,
    tools: HashMap<String, Box<dyn ToolHandler>>,
    resources: HashMap<String, Box<dyn ResourceHandler>>,
    prompts: HashMap<String, Box<dyn PromptHandler>>,
}

pub struct MCPServerHandle {
    pub handle: JoinHandle<()>,
    command_tx: mpsc::Sender<CommandTuple>,
    notification_tx: broadcast::Sender<ServerNotification>,
}

impl MCPServer {
    /// Create a new MCP server
    pub fn new(name: String, version: String) -> Self {
        Self {
            server_info: Implementation { name, version },
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(true),
                }),
                ..ServerCapabilities::default()
            },
            tools: HashMap::new(),
            resources: HashMap::new(),
            prompts: HashMap::new(),
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
    pub fn register_tool(&mut self, handler: Box<dyn ToolHandler>) {
        let metadata = handler.metadata();
        let name = metadata.name.clone();
        self.tools.insert(name, handler);
    }

    /// Register a resource handler
    pub fn register_resource(&mut self, handler: Box<dyn ResourceHandler>) {
        let metadata = handler.metadata();
        let uri = metadata.uri.clone();
        self.resources.insert(uri, handler);
    }

    /// Register a prompt handler
    pub fn register_prompt(&mut self, handler: Box<dyn PromptHandler>) {
        let metadata = handler.metadata();
        let name = metadata.name.clone();
        self.prompts.insert(name, handler);
    }

    /// Remove a tool handler
    pub async fn remove_tool(&mut self, name: &str) -> Option<Box<dyn ToolHandler>> {
        self.tools.remove(name)
    }

    /// Send a server notification - used internally by the server loop
    fn send_server_notification(
        &self,
        notification_tx: &broadcast::Sender<ServerNotification>,
        notification: ServerNotification,
    ) {
        // TODO Skip sending notifications if specific server capabilities are not enabled
        if let Err(e) = notification_tx.send(notification.clone()) {
            error!(
                "Failed to send server notification {:?}: {}",
                notification, e
            );
        }
    }
}

impl MCPServerHandle {
    /// Start serving connections using the provided transport, returning a handle for runtime operations
    pub async fn new(mut server: MCPServer, mut transport: Box<dyn Transport>) -> Result<Self> {
        transport.connect().await?;
        let stream = transport.framed()?;
        let (mut sink_tx, mut stream_rx) = stream.split();

        info!("MCP server started");
        let (notification_tx, mut notification_rx) = broadcast::channel(100);
        let (command_tx, mut command_rx) = mpsc::channel::<CommandTuple>(100);

        // Clone notification_tx for the handle
        let notification_tx_handle = notification_tx.clone();

        // Start the main server loop in a background task
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Handle incoming messages from client
                    result = stream_rx.next() => {
                        match result {
                            Some(Ok(message)) => {
                                if let Err(e) = server.handle_message(message, &mut sink_tx).await {
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

                    // Handle runtime registration commands
                    command_tuple = command_rx.recv() => {
                        match command_tuple {
                            None => {
                                info!("Command channel closed, stopping server");
                                break;
                            }
                            Some((command, tx)) => {
                                match command {
                                    ServerCommand::RegisterTool(handler) => {
                                        let metadata = handler.metadata();
                                        let name = metadata.name.clone();
                                        server.tools.insert(name, handler);
                                        server.send_server_notification(&notification_tx, ServerNotification::ToolListChanged);
                                        // info!("Tool '{:?}' registered successfully", name);
                                        if let Err(e) = tx.send(Ok(())) {
                                            error!("Failed to send response for tool registration: {:?}", e);
                                        }
                                    }
                                    ServerCommand::RemoveTool(name) => {
                                        let result = server.tools.remove(&name);
                                        if result.is_some() {
                                            server.send_server_notification(&notification_tx, ServerNotification::ToolListChanged);
                                            info!("Tool '{}' removed successfully", name);
                                        } else {
                                            warn!("Tool '{}' not found for removal", name);
                                        }
                                        // Always return Ok, regardless of whether tool existed
                                        if let Err(e) = tx.send(Ok(())) {
                                            error!("Failed to send response for tool removal: {:?}", e);
                                        }
                                    }
                                }
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
        });

        Ok(MCPServerHandle {
            handle,
            command_tx,
            notification_tx: notification_tx_handle,
        })
    }
}

impl MCPServer {
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
            "ping" => {
                info!("Server received ping request, sending automatic response");
                Ok(serde_json::json!({}))
            }
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
        let tool_list: Vec<Tool> = self.tools.values().map(|h| h.metadata()).collect();

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

        let handler =
            self.tools
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
        let resource_list: Vec<Resource> = self.resources.values().map(|h| h.metadata()).collect();

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

        let handler =
            self.resources
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
        let prompt_list: Vec<Prompt> = self.prompts.values().map(|h| h.metadata()).collect();

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

        let handler = self.prompts.get(&params.name).ok_or_else(|| {
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

impl MCPServerHandle {
    /// Register a tool handler at runtime
    pub async fn register_tool(&self, handler: Box<dyn ToolHandler>) -> Result<()> {
        self.send_command(ServerCommand::RegisterTool(handler))
            .await
    }

    /// Remove a tool handler at runtime
    pub async fn remove_tool(&self, name: &str) -> Result<()> {
        self.send_command(ServerCommand::RemoveTool(name.to_string()))
            .await
    }

    async fn send_command(&self, command: ServerCommand) -> Result<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx.send((command, tx)).await.map_err(|e| {
            MCPError::InternalError(format!("Failed to send command to server: {}", e))
        })?;
        rx.await.map_err(|e| {
            MCPError::InternalError(format!("Failed to receive command response: {}", e))
        })?
    }

    pub async fn stop(self) -> Result<()> {
        // Signal the server to stop by dropping the command channel
        drop(self.command_tx);

        // Wait for the server task to complete
        self.handle
            .await
            .map_err(|e| MCPError::InternalError(format!("Server task failed: {}", e)))?;
        Ok(())
    }

    /// Send a server notification
    pub fn send_server_notification(&self, notification: ServerNotification) {
        // TODO Skip sending notifications if specific server capabilities are not enabled
        if let Err(e) = self.notification_tx.send(notification.clone()) {
            error!(
                "Failed to send server notification {:?}: {}",
                notification, e
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::DuplexStream;
    use tokio::time::timeout;
    use tokio::time::Duration;

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

    // Simple test transport for unit tests
    struct SimpleTestTransport {
        stream: Option<DuplexStream>,
    }

    impl SimpleTestTransport {
        fn new(stream: DuplexStream) -> Self {
            Self {
                stream: Some(stream),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::transport::Transport for SimpleTestTransport {
        async fn connect(&mut self) -> Result<()> {
            Ok(())
        }

        fn framed(mut self: Box<Self>) -> Result<Box<dyn crate::transport::TransportStream>> {
            let stream = self.stream.take().unwrap();
            Ok(Box::new(tokio_util::codec::Framed::new(
                stream,
                crate::codec::JsonRpcCodec::new(),
            )))
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
        let server = MCPServer::new("test-server".to_string(), "1.0.0".to_string());
        let (client, server_stream) = tokio::io::duplex(1024);
        let transport = Box::new(SimpleTestTransport::new(server_stream));

        let server_handle = MCPServerHandle::new(server, transport).await.unwrap();
        let mut notification_rx = server_handle.notification_tx.subscribe();

        // Register a tool
        let tool_handler = Box::new(MockToolHandler {
            name: "test-tool".to_string(),
        });
        let registered = server_handle.register_tool(tool_handler).await;
        assert!(registered.is_ok());

        // Check that we received a notification
        let notification = timeout(Duration::from_millis(100), notification_rx.recv())
            .await
            .expect("Should receive notification within timeout")
            .expect("Should receive notification successfully");

        assert!(matches!(notification, ServerNotification::ToolListChanged));

        // Clean up
        drop(client);
        let _ = server_handle.stop().await;
    }

    #[tokio::test]
    async fn test_tool_removal_sends_notification() {
        let mut server = MCPServer::new("test-server".to_string(), "1.0.0".to_string());

        // First register a tool
        let tool_handler = Box::new(MockToolHandler {
            name: "test-tool".to_string(),
        });
        server.register_tool(tool_handler);

        // Run the server
        let (client, server_stream) = tokio::io::duplex(1024);
        let transport = Box::new(SimpleTestTransport::new(server_stream));
        let server_handle = MCPServerHandle::new(server, transport).await.unwrap();
        let mut notification_rx = server_handle.notification_tx.subscribe();

        // Remove the tool during runtime
        let removed = server_handle.remove_tool("test-tool").await;
        assert!(removed.is_ok());

        // Check that we received another notification
        let notification = timeout(Duration::from_millis(100), notification_rx.recv())
            .await
            .expect("Should receive notification within timeout")
            .expect("Should receive notification successfully");

        assert!(matches!(notification, ServerNotification::ToolListChanged));

        // Clean up
        drop(client);
        let _ = server_handle.stop().await;
    }

    #[tokio::test]
    async fn test_remove_nonexistent_tool_no_notification() {
        let server = MCPServer::new("test-server".to_string(), "1.0.0".to_string());
        let (client, server_stream) = tokio::io::duplex(1024);
        let transport = Box::new(SimpleTestTransport::new(server_stream));
        let server_handle = MCPServerHandle::new(server, transport).await.unwrap();
        let mut notification_rx = server_handle.notification_tx.subscribe();

        // Try to remove a tool that doesn't exist
        let removed = server_handle.remove_tool("nonexistent-tool").await;
        assert!(removed.is_ok());

        // Check that we don't receive any notification
        let result = timeout(Duration::from_millis(100), notification_rx.recv()).await;
        assert!(
            result.is_err(),
            "Should not receive notification for nonexistent tool removal"
        );

        // Clean up
        drop(client);
        let _ = server_handle.stop().await;
    }

    async fn setup_test_client_server() -> (crate::client::MCPClient, MCPServerHandle) {
        use crate::client::MCPClient;
        use crate::schema::{ClientCapabilities, Implementation};
        use crate::transport::TestTransport;

        let (client_transport, server_transport) = TestTransport::create_pair();

        let server = MCPServer::new("test-server".to_string(), "1.0.0".to_string());
        let server_handle = MCPServerHandle::new(server, server_transport)
            .await
            .expect("Failed to start server");

        let mut client = MCPClient::new();
        client
            .connect(client_transport)
            .await
            .expect("Failed to connect");

        let client_info = Implementation {
            name: "test-client".to_string(),
            version: "1.0.0".to_string(),
        };
        client
            .initialize(client_info, ClientCapabilities::default())
            .await
            .expect("Failed to initialize");

        (client, server_handle)
    }

    #[tokio::test]
    async fn test_server_ping_response() {
        let (mut client, _server) = setup_test_client_server().await;
        client.ping().await.expect("Server should respond to ping");
    }

    #[tokio::test]
    async fn test_bidirectional_ping_sequence() {
        let (mut client, _server) = setup_test_client_server().await;

        for i in 0..10 {
            client
                .ping()
                .await
                .unwrap_or_else(|_| panic!("Ping {} failed", i));
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn test_server_handles_numeric_request_id() {
        let (mut client, _server) = setup_test_client_server().await;
        client.ping().await.expect("Server should handle ping");
    }
}
