use futures::{stream::SplitSink, SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::{
    connection::{Connection, ConnectionContext},
    error::{Error, Result},
    schema::{self, *},
    transport::{Transport, TransportStream},
};

/// Factory function type for creating Connection instances
pub type ConnectionFactory = Box<dyn Fn() -> Box<dyn Connection> + Send + Sync>;

/// MCP Server implementation
#[derive(Default)]
pub struct Server {
    capabilities: ServerCapabilities,
    connection_factory: Option<ConnectionFactory>,
}

pub struct ServerHandle {
    pub handle: JoinHandle<()>,
    notification_tx: broadcast::Sender<ServerNotification>,
}

impl Server {
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

    /// Serve a single connection using the provided transport
    /// This is a convenience method that starts the server and waits for completion
    pub async fn serve(self, transport: Box<dyn Transport>) -> Result<()> {
        let handle = ServerHandle::new(self, transport).await?;
        handle.stop().await
    }

    /// Serve connections from stdin/stdout
    /// This is a convenience method for the common stdio use case
    pub async fn serve_stdio(self) -> Result<()> {
        let transport = Box::new(crate::transport::StdioTransport::new());
        self.serve(transport).await
    }

    /// Serve using generic AsyncRead and AsyncWrite streams
    /// This is a convenience method that creates a StreamTransport from the provided streams
    pub async fn serve_stream<R, W>(self, reader: R, writer: W) -> Result<()>
    where
        R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
        W: tokio::io::AsyncWrite + Send + Sync + Unpin + 'static,
    {
        let duplex = crate::transport::GenericDuplex::new(reader, writer);
        let transport = Box::new(crate::transport::StreamTransport::new(duplex));
        self.serve(transport).await
    }

    /// Serve TCP connections by accepting them in a loop
    /// This is a convenience method for the common TCP server use case
    pub async fn serve_tcp(self, addr: impl tokio::net::ToSocketAddrs) -> Result<()> {
        use std::sync::Arc;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        info!("MCP server listening on {}", local_addr);

        // Convert connection factory to Arc for sharing across tasks
        let connection_factory = Arc::new(self.connection_factory);
        let capabilities = Arc::new(self.capabilities);

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    info!("New connection from {}", peer_addr);

                    // Clone Arc references for the spawned task
                    let factory = connection_factory.clone();
                    let caps = capabilities.clone();

                    // Handle each connection in a separate task
                    tokio::spawn(async move {
                        // Create a new connection factory that clones the Arc'd factory
                        let server = Server {
                            capabilities: (*caps).clone(),
                            connection_factory: factory.as_ref().as_ref().map(|_f| {
                                let factory = factory.clone();
                                Box::new(move || {
                                    // Call the original factory through the Arc
                                    factory.as_ref().as_ref().unwrap()()
                                }) as ConnectionFactory
                            }),
                        };

                        let transport = Box::new(crate::transport::StreamTransport::new(stream));

                        match server.serve(transport).await {
                            Ok(()) => info!("Connection from {} closed", peer_addr),
                            Err(e) => error!("Error handling connection from {}: {}", peer_addr, e),
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    /// Start a TCP listener and return a handle for advanced control
    /// Returns a task handle for the accept loop
    ///
    /// Note: This returns only the JoinHandle, not the listener itself,
    /// as the listener is moved into the spawned task.
    pub async fn listen_tcp(
        self,
        addr: impl tokio::net::ToSocketAddrs,
    ) -> Result<JoinHandle<Result<()>>> {
        use std::sync::Arc;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        info!("MCP server listening on {}", local_addr);

        // Convert to Arc for sharing
        let connection_factory = Arc::new(self.connection_factory);
        let capabilities = Arc::new(self.capabilities);

        let handle = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer_addr)) => {
                        info!("New connection from {}", peer_addr);

                        let factory = connection_factory.clone();
                        let caps = capabilities.clone();

                        tokio::spawn(async move {
                            // Create a new connection factory that clones the Arc'd factory
                            let server = Server {
                                capabilities: (*caps).clone(),
                                connection_factory: factory.as_ref().as_ref().map(|_f| {
                                    let factory = factory.clone();
                                    Box::new(move || {
                                        // Call the original factory through the Arc
                                        factory.as_ref().as_ref().unwrap()()
                                    }) as ConnectionFactory
                                }),
                            };

                            let transport =
                                Box::new(crate::transport::StreamTransport::new(stream));

                            match server.serve(transport).await {
                                Ok(()) => info!("Connection from {} closed", peer_addr),
                                Err(e) => {
                                    error!("Error handling connection from {}: {}", peer_addr, e)
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("Failed to accept connection: {}", e);
                        return Err(Error::from(e));
                    }
                }
            }
        });

        Ok(handle)
    }
}

impl ServerHandle {
    /// Start serving connections using the provided transport, returning a handle for runtime operations
    pub async fn new(server: Server, mut transport: Box<dyn Transport>) -> Result<Self> {
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

        Ok(ServerHandle {
            handle,
            notification_tx: notification_tx_handle,
        })
    }

    /// Create a ServerHandle using generic AsyncRead and AsyncWrite streams
    /// This is a convenience method that creates a StreamTransport from the provided streams
    pub async fn from_stream<R, W>(server: Server, reader: R, writer: W) -> Result<Self>
    where
        R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
        W: tokio::io::AsyncWrite + Send + Sync + Unpin + 'static,
    {
        let duplex = crate::transport::GenericDuplex::new(reader, writer);
        let transport = Box::new(crate::transport::StreamTransport::new(duplex));
        Self::new(server, transport).await
    }

    pub async fn stop(self) -> Result<()> {
        // Wait for the server task to complete
        self.handle
            .await
            .map_err(|e| Error::InternalError(format!("Server task failed: {e}")))?;
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
            tracing::info!("Server sending response: {:?}", response_message);
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
            // Check if error has a specific JSONRPC response
            if let Some(jsonrpc_error) = e.to_jsonrpc_response(request.id.clone()) {
                JSONRPCMessage::Error(jsonrpc_error)
            } else {
                // For all other errors, use INTERNAL_ERROR
                JSONRPCMessage::Error(JSONRPCError {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id: request.id,
                    error: ErrorObject {
                        code: INTERNAL_ERROR,
                        message: e.to_string(),
                        data: None,
                    },
                })
            }
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
                Error::InvalidParams("initialize: Missing required parameters".to_string())
            })?;
            let params = serde_json::from_value::<InitializeParams>(p)
                .map_err(|e| Error::InvalidParams(format!("initialize: {e}")))?;
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
                Error::InvalidParams("tools/call: Missing required parameters".to_string())
            })?;
            let params = serde_json::from_value::<CallToolParams>(p)
                .map_err(|e| Error::InvalidParams(format!("tools/call: {e}")))?;
            let arguments = params
                .arguments
                .map(serde_json::to_value)
                .transpose()
                .map_err(|e| {
                    Error::InvalidParams(format!("tools/call: Invalid arguments format: {e}"))
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
        "resources/templates/list" => connection
            .resources_templates_list()
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        "resources/read" => {
            let p = params.ok_or_else(|| {
                Error::InvalidParams("resources/read: Missing required parameters".to_string())
            })?;
            let params = serde_json::from_value::<ReadResourceParams>(p)
                .map_err(|e| Error::InvalidParams(format!("resources/read: {e}")))?;
            connection
                .resources_read(params.uri)
                .await
                .and_then(|result| serde_json::to_value(result).map_err(Into::into))
        }
        "resources/subscribe" => {
            let p = params.ok_or_else(|| {
                Error::InvalidParams("resources/subscribe: Missing required parameters".to_string())
            })?;
            let params = serde_json::from_value::<HashMap<String, String>>(p)
                .map_err(|e| Error::InvalidParams(format!("resources/subscribe: {e}")))?;
            let uri = params.get("uri").ok_or_else(|| {
                Error::InvalidParams("resources/subscribe: Missing uri parameter".to_string())
            })?;
            connection
                .resources_subscribe(uri.clone())
                .await
                .map(|_| serde_json::json!({}))
        }
        "resources/unsubscribe" => {
            let p = params.ok_or_else(|| {
                Error::InvalidParams(
                    "resources/unsubscribe: Missing required parameters".to_string(),
                )
            })?;
            let params = serde_json::from_value::<HashMap<String, String>>(p)
                .map_err(|e| Error::InvalidParams(format!("resources/unsubscribe: {e}")))?;
            let uri = params.get("uri").ok_or_else(|| {
                Error::InvalidParams("resources/unsubscribe: Missing uri parameter".to_string())
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
                Error::InvalidParams("prompts/get: Missing required parameters".to_string())
            })?;
            let params = serde_json::from_value::<GetPromptParams>(p)
                .map_err(|e| Error::InvalidParams(format!("prompts/get: {e}")))?;
            connection
                .prompts_get(params.name, params.arguments)
                .await
                .and_then(|result| serde_json::to_value(result).map_err(Into::into))
        }
        "completion/complete" => {
            let p = params.ok_or_else(|| {
                Error::InvalidParams("completion/complete: Missing required parameters".to_string())
            })?;
            let params = serde_json::from_value::<CompleteParams>(p)
                .map_err(|e| Error::InvalidParams(format!("completion/complete: {e}")))?;
            connection
                .completion_complete(params.reference, params.argument)
                .await
                .and_then(|result| serde_json::to_value(result).map_err(Into::into))
        }
        "logging/setLevel" => {
            let p = params.ok_or_else(|| {
                Error::InvalidParams("logging/setLevel: Missing required parameters".to_string())
            })?;
            let params = serde_json::from_value::<HashMap<String, LoggingLevel>>(p)
                .map_err(|e| Error::InvalidParams(format!("logging/setLevel: {e}")))?;
            let level = params.get("level").ok_or_else(|| {
                Error::InvalidParams("logging/setLevel: Missing level parameter".to_string())
            })?;
            connection
                .logging_set_level(*level)
                .await
                .map(|_| serde_json::json!({}))
        }
        _ => Err(Error::MethodNotFound(request.request.method.clone())),
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
