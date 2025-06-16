use futures::{stream::SplitSink, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::{
    connection::{Connection, ConnectionContext},
    error::{MCPError, Result},
    schema::{self, *},
    transport::{Transport, TransportStream},
};

/// Factory function type for creating Connection instances
pub type ConnectionFactory = Box<dyn Fn() -> Box<dyn Connection> + Send + Sync>;

/// MCP Server implementation
#[derive(Default)]
pub struct MCPServer {
    capabilities: ServerCapabilities,
    connection_factory: Option<ConnectionFactory>,
}

pub struct MCPServerHandle {
    pub handle: JoinHandle<()>,
    notification_tx: broadcast::Sender<ServerNotification>,
}

impl MCPServer {
    /// Set the connection factory for creating Connection instances
    pub fn with_connection_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn() -> Box<dyn Connection> + Send + Sync + 'static,
    {
        self.connection_factory = Some(Box::new(factory));
        self
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
}

impl MCPServerHandle {
    /// Start serving connections using the provided transport, returning a handle for runtime operations
    pub async fn new(server: MCPServer, mut transport: Box<dyn Transport>) -> Result<Self> {
        transport.connect().await?;
        let stream = transport.framed()?;
        let (mut sink_tx, mut stream_rx) = stream.split();

        info!("MCP server started");
        let (notification_tx, mut notification_rx) = broadcast::channel(100);

        // Clone notification_tx for the handle
        let notification_tx_handle = notification_tx.clone();

        // Create connection instance
        let mut connection: Option<Box<dyn Connection>> =
            if let Some(factory) = &server.connection_factory {
                Some(factory())
            } else {
                None
            };

        // Initialize connection if present
        if let Some(conn) = &mut connection {
            let context = ConnectionContext {
                notification_tx: notification_tx.clone(),
            };
            conn.on_connect(context).await?;
        }

        // Start the main server loop in a background task
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Handle incoming messages from client
                    result = stream_rx.next() => {
                        match result {
                            Some(Ok(message)) => {
                                if let Some(conn) = &mut connection {
                                    if let Err(e) = handle_message_with_connection(conn, message, &mut sink_tx).await {
                                        error!("Error handling message: {}", e);
                                    }
                                } else {
                                    error!("No connection factory provided - unable to handle messages");
                                    break;
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
                                let jsonrpc_notification = create_jsonrpc_notification(notification);
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

            // Clean up connection
            if let Some(mut conn) = connection {
                if let Err(e) = conn.on_disconnect().await {
                    error!("Error during connection disconnect: {}", e);
                }
            }

            info!("MCP server stopped");
        });

        Ok(MCPServerHandle {
            handle,
            notification_tx: notification_tx_handle,
        })
    }

    pub async fn stop(self) -> Result<()> {
        // Wait for the server task to complete
        self.handle
            .await
            .map_err(|e| MCPError::InternalError(format!("Server task failed: {e}")))?;
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

/// Handle a message using the Connection trait
async fn handle_message_with_connection(
    connection: &mut Box<dyn Connection>,
    message: JSONRPCMessage,
    sink: &mut SplitSink<Box<dyn TransportStream>, JSONRPCMessage>,
) -> Result<()> {
    match message {
        JSONRPCMessage::Request(request) => {
            let response_message = handle_request(connection, request.clone()).await;
            sink.send(response_message).await?;
        }
        JSONRPCMessage::Notification(notification) => {
            handle_notification(connection, notification).await?;
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

/// Handle a request using the Connection trait and convert result to JSONRPCMessage
async fn handle_request(
    connection: &mut Box<dyn Connection>,
    request: JSONRPCRequest,
) -> JSONRPCMessage {
    let result = handle_request_inner(connection, request.clone()).await;

    match result {
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

/// Inner handler that returns Result<serde_json::Value>
async fn handle_request_inner(
    connection: &mut Box<dyn Connection>,
    request: JSONRPCRequest,
) -> Result<serde_json::Value> {
    // Convert RequestParams to serde_json::Value
    let params = request
        .request
        .params
        .map(|p| serde_json::to_value(p.other))
        .transpose()?;

    match request.request.method.as_str() {
        "initialize" => {
            let p = params.ok_or_else(|| {
                MCPError::invalid_params("initialize", "Missing required parameters")
            })?;
            let params = serde_json::from_value::<InitializeParams>(p)
                .map_err(|e| MCPError::invalid_params("initialize", e.to_string()))?;
            connection
                .initialize(
                    params.protocol_version,
                    params.capabilities,
                    params.client_info,
                )
                .await
                .and_then(|result| serde_json::to_value(result).map_err(Into::into))
        }
        "ping" => {
            info!("Server received ping request, sending automatic response");
            connection.ping().await.map(|_| serde_json::json!({}))
        }
        "tools/list" => connection
            .tools_list()
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        "tools/call" => {
            let p = params.ok_or_else(|| {
                MCPError::invalid_params("tools/call", "Missing required parameters")
            })?;
            let params = serde_json::from_value::<CallToolParams>(p)
                .map_err(|e| MCPError::invalid_params("tools/call", e.to_string()))?;
            let arguments = params
                .arguments
                .map(serde_json::to_value)
                .transpose()
                .map_err(|e| {
                    MCPError::invalid_params("tools/call", format!("Invalid arguments format: {e}"))
                })?;
            connection
                .tools_call(params.name, arguments)
                .await
                .and_then(|result| serde_json::to_value(result).map_err(Into::into))
        }
        "resources/list" => connection
            .resources_list()
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        "resources/templates/list" => {
            connection
                .resources_templates_list()
                .await
                .and_then(|result| serde_json::to_value(result).map_err(Into::into))
        }
        "resources/read" => {
            let p = params.ok_or_else(|| {
                MCPError::invalid_params("resources/read", "Missing required parameters")
            })?;
            let params = serde_json::from_value::<ReadResourceParams>(p)
                .map_err(|e| MCPError::invalid_params("resources/read", e.to_string()))?;
            connection
                .resources_read(params.uri)
                .await
                .and_then(|result| serde_json::to_value(result).map_err(Into::into))
        }
        "resources/subscribe" => {
            let p = params.ok_or_else(|| {
                MCPError::invalid_params("resources/subscribe", "Missing required parameters")
            })?;
            let params = serde_json::from_value::<HashMap<String, String>>(p)
                .map_err(|e| MCPError::invalid_params("resources/subscribe", e.to_string()))?;
            let uri = params.get("uri").ok_or_else(|| {
                MCPError::invalid_params("resources/subscribe", "Missing uri parameter")
            })?;
            connection
                .resources_subscribe(uri.clone())
                .await
                .map(|_| serde_json::json!({}))
        }
        "resources/unsubscribe" => {
            let p = params.ok_or_else(|| {
                MCPError::invalid_params("resources/unsubscribe", "Missing required parameters")
            })?;
            let params = serde_json::from_value::<HashMap<String, String>>(p)
                .map_err(|e| MCPError::invalid_params("resources/unsubscribe", e.to_string()))?;
            let uri = params.get("uri").ok_or_else(|| {
                MCPError::invalid_params("resources/unsubscribe", "Missing uri parameter")
            })?;
            connection
                .resources_unsubscribe(uri.clone())
                .await
                .map(|_| serde_json::json!({}))
        }
        "prompts/list" => connection
            .prompts_list()
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        "prompts/get" => {
            let p = params.ok_or_else(|| {
                MCPError::invalid_params("prompts/get", "Missing required parameters")
            })?;
            let params = serde_json::from_value::<GetPromptParams>(p)
                .map_err(|e| MCPError::invalid_params("prompts/get", e.to_string()))?;
            connection
                .prompts_get(params.name, params.arguments)
                .await
                .and_then(|result| serde_json::to_value(result).map_err(Into::into))
        }
        "completion/complete" => {
            let p = params.ok_or_else(|| {
                MCPError::invalid_params("completion/complete", "Missing required parameters")
            })?;
            let params = serde_json::from_value::<CompleteParams>(p)
                .map_err(|e| MCPError::invalid_params("completion/complete", e.to_string()))?;
            connection
                .completion_complete(params.reference, params.argument)
                .await
                .and_then(|result| serde_json::to_value(result).map_err(Into::into))
        }
        "logging/setLevel" => {
            let p = params.ok_or_else(|| {
                MCPError::invalid_params("logging/setLevel", "Missing required parameters")
            })?;
            let params = serde_json::from_value::<HashMap<String, LoggingLevel>>(p)
                .map_err(|e| MCPError::invalid_params("logging/setLevel", e.to_string()))?;
            let level = params.get("level").ok_or_else(|| {
                MCPError::invalid_params("logging/setLevel", "Missing level parameter")
            })?;
            connection
                .logging_set_level(*level)
                .await
                .map(|_| serde_json::json!({}))
        }
        _ => Err(MCPError::MethodNotFound(request.request.method.clone())),
    }
}

/// Handle a notification using the Connection trait
async fn handle_notification(
    _connection: &mut Box<dyn Connection>,
    notification: JSONRPCNotification,
) -> Result<()> {
    debug!(
        "Received notification: {}",
        notification.notification.method
    );

    // Convert notification params if needed
    let _params = notification
        .notification
        .params
        .map(|p| serde_json::to_value(p.other))
        .transpose()?;

    match notification.notification.method.as_str() {
        "notifications/cancelled" => {
            // Handle cancellation notification if needed
            debug!("Received cancellation notification");
        }
        "notifications/progress" => {
            // Handle progress notification if needed
            debug!("Received progress notification");
        }
        "notifications/initialized" => {
            // Client has completed initialization
            debug!("Client initialization complete");
        }
        "notifications/roots/list_changed" => {
            // Roots list has changed
            debug!("Roots list changed");
        }
        _ => {
            debug!(
                "Unknown notification method: {}",
                notification.notification.method
            );
        }
    }

    Ok(())
}

// Helper function to create JSONRPC notifications
fn create_jsonrpc_notification(notification: ServerNotification) -> JSONRPCNotification {
    JSONRPCNotification {
        jsonrpc: JSONRPC_VERSION.to_string(),
        notification: Notification {
            method: match &notification {
                ServerNotification::ToolListChanged => "notifications/tools/list_changed",
                ServerNotification::ResourceListChanged => "notifications/resources/list_changed",
                ServerNotification::PromptListChanged => "notifications/prompts/list_changed",
                ServerNotification::ResourceUpdated { .. } => "notifications/resources/updated",
                ServerNotification::LoggingMessage { .. } => "notifications/message",
                ServerNotification::Progress { .. } => "notifications/progress",
                ServerNotification::Cancelled { .. } => "notifications/cancelled",
            }
            .to_string(),
            params: match notification {
                ServerNotification::ToolListChanged
                | ServerNotification::ResourceListChanged
                | ServerNotification::PromptListChanged => None,
                ServerNotification::ResourceUpdated { uri } => {
                    let mut params = HashMap::new();
                    params.insert("uri".to_string(), serde_json::Value::String(uri));
                    Some(NotificationParams {
                        meta: None,
                        other: params,
                    })
                }
                ServerNotification::LoggingMessage {
                    level,
                    logger,
                    data,
                } => {
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
                }
                ServerNotification::Progress {
                    progress_token,
                    progress,
                    total,
                    message,
                } => {
                    let mut params = HashMap::new();
                    params.insert(
                        "progressToken".to_string(),
                        serde_json::to_value(progress_token).unwrap(),
                    );
                    params.insert(
                        "progress".to_string(),
                        serde_json::Value::Number(serde_json::Number::from_f64(progress).unwrap()),
                    );
                    if let Some(total) = total {
                        params.insert(
                            "total".to_string(),
                            serde_json::Value::Number(serde_json::Number::from_f64(total).unwrap()),
                        );
                    }
                    if let Some(message) = message {
                        params.insert("message".to_string(), serde_json::Value::String(message));
                    }
                    Some(NotificationParams {
                        meta: None,
                        other: params,
                    })
                }
                ServerNotification::Cancelled { request_id, reason } => {
                    let mut params = HashMap::new();
                    params.insert(
                        "requestId".to_string(),
                        serde_json::to_value(request_id).unwrap(),
                    );
                    if let Some(reason) = reason {
                        params.insert("reason".to_string(), serde_json::Value::String(reason));
                    }
                    Some(NotificationParams {
                        meta: None,
                        other: params,
                    })
                }
            },
        },
    }
}

// Add CompleteParams struct
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompleteParams {
    #[serde(rename = "ref")]
    reference: Reference,
    argument: ArgumentInfo,
}
