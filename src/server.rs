use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use schema::*;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::{
    error::{MCPError, Result},
    schema,
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
}

impl MCPServer {
    /// Create a new MCP server
    pub fn new(name: String, version: String) -> Self {
        Self {
            server_info: Implementation { name, version },
            capabilities: ServerCapabilities::default(),
            tools: Arc::new(Mutex::new(HashMap::new())),
            resources: Arc::new(Mutex::new(HashMap::new())),
            prompts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Set server capabilities
    pub fn with_capabilities(mut self, capabilities: ServerCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Register a tool handler
    pub async fn register_tool(&mut self, handler: Box<dyn ToolHandler>) {
        let metadata = handler.metadata();
        let name = metadata.name.clone();
        self.tools.lock().await.insert(name, handler);
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

    /// Start serving connections using the provided transport
    pub async fn serve(&self, mut transport: Box<dyn Transport>) -> Result<()> {
        transport.connect().await?;
        let mut stream = transport.framed()?;

        info!("MCP server started");

        while let Some(result) = stream.next().await {
            match result {
                Ok(message) => {
                    if let Err(e) = self.handle_message(message, &mut stream).await {
                        error!("Error handling message: {}", e);
                    }
                }
                Err(e) => {
                    error!("Error reading message: {}", e);
                    break;
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
        stream: &mut Box<dyn TransportStream>,
    ) -> Result<()> {
        match message {
            JSONRPCMessage::Request(request) => {
                let response_message = self.handle_request(request.clone()).await;
                stream.send(response_message).await?;
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
                    .map(|args| serde_json::to_value(args))
                    .transpose()
                    .map_err(|e| {
                        MCPError::invalid_params(
                            "tools/call",
                            format!("Invalid arguments format: {}", e),
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
            _ => MCPError::handler_error("resource", format!("Failed to read '{}': {}", uri, e)),
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
                    .map(|args| serde_json::to_value(args))
                    .transpose()
                    .map_err(|e| {
                        MCPError::invalid_params(
                            "prompts/get",
                            format!("Invalid arguments format: {}", e),
                        )
                    })?,
            )
            .await
            .map_err(|e| {
                MCPError::handler_error(
                    "prompt",
                    format!("Failed to get prompt '{}': {}", prompt_name, e),
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

    #[test]
    fn test_server_creation() {
        let server = MCPServer::new("test-server".to_string(), "1.0.0".to_string());
        assert_eq!(server.server_info.name, "test-server");
        assert_eq!(server.server_info.version, "1.0.0");
    }
}
