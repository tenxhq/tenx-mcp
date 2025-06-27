use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::{
    connection::ServerConn,
    context::ServerCtx,
    error::{Error, Result},
    http::HttpServerTransport,
    jsonrpc::create_jsonrpc_notification,
    schema::{self, *},
    transport::{GenericDuplex, StdioTransport, StreamTransport, Transport},
};

/// MCP Server implementation
pub struct Server<F = fn() -> Box<dyn ServerConn>> {
    capabilities: ServerCapabilities,
    connection_factory: Option<F>,
}

impl Default for Server<fn() -> Box<dyn ServerConn>> {
    fn default() -> Self {
        Self {
            capabilities: ServerCapabilities::default(),
            connection_factory: None,
        }
    }
}

impl<F> Server<F>
where
    F: Fn() -> Box<dyn ServerConn> + Send + Sync + 'static,
{
    /// Set the connection factory for creating Connection instances
    pub(crate) fn with_connection_factory<G>(self, factory: G) -> Server<G>
    where
        G: Fn() -> Box<dyn ServerConn> + Send + Sync + 'static,
    {
        Server {
            capabilities: self.capabilities,
            connection_factory: Some(factory),
        }
    }

    /// Set a connection factory that creates concrete connection types
    /// This method automatically boxes the connection type for you.
    pub fn with_connection<C, G>(
        self,
        factory: G,
    ) -> Server<impl Fn() -> Box<dyn ServerConn> + Clone + Send + Sync + 'static>
    where
        C: ServerConn + 'static,
        G: Fn() -> C + Clone + Send + Sync + 'static,
    {
        Server {
            capabilities: self.capabilities,
            connection_factory: Some(move || Box::new(factory()) as Box<dyn ServerConn>),
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

    /// Serve a single connection using the provided transport
    /// This is a convenience method that starts the server and waits for completion
    pub(crate) async fn serve(self, transport: Box<dyn Transport>) -> Result<()> {
        let handle = ServerHandle::new(self, transport).await?;
        handle
            .handle
            .await
            .map_err(|e| Error::InternalError(format!("Server task failed: {e}")))
    }

    /// Serve connections from stdin/stdout
    /// This is a convenience method for the common stdio use case
    pub async fn serve_stdio(self) -> Result<()> {
        let transport = Box::new(StdioTransport::new());
        self.serve(transport).await
    }

    /// Serve using generic AsyncRead and AsyncWrite streams
    /// This is a convenience method that creates a StreamTransport from the provided streams
    pub async fn serve_stream<R, W>(self, reader: R, writer: W) -> Result<()>
    where
        R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
        W: tokio::io::AsyncWrite + Send + Sync + Unpin + 'static,
    {
        let duplex = GenericDuplex::new(reader, writer);
        let transport = Box::new(StreamTransport::new(duplex));
        self.serve(transport).await
    }

    /// Serve TCP connections by accepting them in a loop
    /// This is a convenience method for the common TCP server use case
    pub async fn serve_tcp(self, addr: impl tokio::net::ToSocketAddrs) -> Result<()>
    where
        F: Clone,
    {
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
                        // Create a new server with cloned factory
                        let server = Server {
                            capabilities: (*caps).clone(),
                            connection_factory: factory.as_ref().clone(),
                        };

                        let transport = Box::new(StreamTransport::new(stream));

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

    /// Serve HTTP connections
    /// This is a convenience method for the common HTTP server use case
    /// Returns a ServerHandle that can be used to stop the server
    pub async fn serve_http(self, addr: impl AsRef<str>) -> Result<ServerHandle> {
        let mut http_transport = HttpServerTransport::new(addr.as_ref());
        http_transport.start().await?;
        let bound_addr = http_transport.bind_addr.clone();

        let mut handle = ServerHandle::from_transport(self, Box::new(http_transport)).await?;
        handle.bound_addr = Some(bound_addr);
        Ok(handle)
    }
}

pub struct ServerHandle {
    pub handle: JoinHandle<()>,
    notification_tx: broadcast::Sender<ServerNotification>,
    shutdown_token: CancellationToken,
    /// The actual bound address (for servers that bind to a network port)
    pub bound_addr: Option<String>,
}

impl ServerHandle {
    /// Start serving connections using the provided transport, returning a handle for runtime operations
    pub(crate) async fn new<F>(server: Server<F>, mut transport: Box<dyn Transport>) -> Result<Self>
    where
        F: Fn() -> Box<dyn ServerConn> + Send + Sync + 'static,
    {
        transport.connect().await?;
        let remote_addr = transport.remote_addr();
        let stream = transport.framed()?;
        let (sink_tx, mut stream_rx) = stream.split();

        info!("MCP server started");
        let (notification_tx, mut notification_rx) = broadcast::channel(100);

        // Channel for queueing responses to be sent
        let (response_tx, mut response_rx) =
            tokio::sync::mpsc::unbounded_channel::<JSONRPCMessage>();

        // Wrap the sink in an Arc<Mutex> for sharing
        let sink_tx = Arc::new(Mutex::new(sink_tx));

        // Clone notification_tx for the handle
        let notification_tx_handle = notification_tx.clone();

        // Create connection instance wrapped in Arc for shared access
        let connection: Option<Arc<Box<dyn ServerConn>>> =
            if let Some(factory) = &server.connection_factory {
                Some(Arc::new(factory()))
            } else {
                None
            };

        // Create a single ServerCtx instance that will be used throughout the connection
        let server_ctx = ServerCtx::new(notification_tx.clone(), Some(sink_tx.clone()));

        // Create shutdown token for coordinating shutdown
        let shutdown_token = CancellationToken::new();
        let shutdown_token_task = shutdown_token.clone();

        // Track whether we've called on_connect yet
        let mut connected = false;

        // Start the main server loop in a background task
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Check for shutdown signal
                    _ = shutdown_token_task.cancelled() => {
                        info!("Server received shutdown signal");
                        break;
                    }
                    // Handle incoming messages from client
                    result = stream_rx.next() => {
                        match result {
                            Some(Ok(message)) => {
                                if let Some(conn) = &connection {
                                    // Call on_connect when we receive the first message from client
                                    if !connected {
                                        if let Err(e) = conn.on_connect(&server_ctx, &remote_addr).await {
                                            error!("Error during on_connect: {}", e);
                                            break;
                                        }
                                        connected = true;
                                    }

                                    // Handle responses and errors from client specially
                                    match &message {
                                        JSONRPCMessage::Response(response) => {
                                            tracing::info!("Server received response from client: {:?}", response.id);
                                            server_ctx.handle_client_response(response.clone()).await;
                                        }
                                        JSONRPCMessage::Error(error) => {
                                            tracing::info!("Server received error from client: {:?}", error.id);
                                            server_ctx.handle_client_error(error.clone()).await;
                                        }
                                        _ => {
                                            if let Err(e) = handle_message_with_connection(conn.clone(), message, response_tx.clone(), &server_ctx).await {
                                                error!("Error handling message: {}", e);
                                            }
                                        }
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
                                {
                                    let mut sink = sink_tx.lock().await;
                                    if let Err(e) = sink.send(JSONRPCMessage::Notification(jsonrpc_notification)).await {
                                        error!("Error sending notification to client: {}", e);
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                debug!("Notification channel closed: {}", e);
                                // This is expected when the server shuts down
                            }
                        }
                    }

                    // Send queued responses to client
                    Some(response) = response_rx.recv() => {
                        let mut sink = sink_tx.lock().await;
                        if let Err(e) = sink.send(response).await {
                            error!("Error sending response to client: {}", e);
                            break;
                        }
                    }
                }
            }

            // Clean up connection
            if let Some(conn) = connection {
                if let Err(e) = conn.on_shutdown().await {
                    error!("Error during server shutdown: {}", e);
                }
            }

            info!("MCP server stopped");
        });

        Ok(ServerHandle {
            handle,
            notification_tx: notification_tx_handle,
            shutdown_token,
            bound_addr: None,
        })
    }

    /// Create a ServerHandle using generic AsyncRead and AsyncWrite streams
    /// This is a convenience method that creates a StreamTransport from the provided streams
    pub async fn from_stream<F, R, W>(server: Server<F>, reader: R, writer: W) -> Result<Self>
    where
        F: Fn() -> Box<dyn ServerConn> + Send + Sync + 'static,
        R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
        W: tokio::io::AsyncWrite + Send + Sync + Unpin + 'static,
    {
        let duplex = GenericDuplex::new(reader, writer);
        let transport = Box::new(StreamTransport::new(duplex));
        Self::new(server, transport).await
    }

    /// Create a ServerHandle from a transport
    /// This allows using any transport implementation
    pub async fn from_transport<F>(server: Server<F>, transport: Box<dyn Transport>) -> Result<Self>
    where
        F: Fn() -> Box<dyn ServerConn> + Send + Sync + 'static,
    {
        Self::new(server, transport).await
    }

    pub async fn stop(self) -> Result<()> {
        // Signal shutdown
        self.shutdown_token.cancel();

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
    connection: Arc<Box<dyn ServerConn>>,
    message: JSONRPCMessage,
    response_tx: tokio::sync::mpsc::UnboundedSender<JSONRPCMessage>,
    context: &ServerCtx,
) -> Result<()> {
    match message {
        JSONRPCMessage::Request(request) => {
            // Process request concurrently
            let conn = connection.clone();
            let ctx = context.clone();
            let tx = response_tx.clone();

            tokio::spawn(async move {
                let response_message = handle_request(&**conn, request.clone(), &ctx).await;
                tracing::info!("Server sending response: {:?}", response_message);

                // Queue the response to be sent
                if let Err(e) = tx.send(response_message) {
                    error!("Failed to queue response: {}", e);
                }
            });
        }
        JSONRPCMessage::Notification(notification) => {
            handle_notification(&**connection, notification, context).await?;
        }
        JSONRPCMessage::Response(_) => {
            // Response handling is done in the main message loop
            debug!("Response handling delegated to main loop");
        }
        JSONRPCMessage::Error(_) => {
            // Error handling is done in the main message loop
            debug!("Error handling delegated to main loop");
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
    connection: &dyn ServerConn,
    request: JSONRPCRequest,
    context: &ServerCtx,
) -> JSONRPCMessage {
    tracing::info!(
        "Server handling request: {:?} method: {}",
        request.id,
        request.request.method
    );
    // Create a context with the request ID
    let ctx_with_request = context.with_request_id(request.id.clone());
    let result = handle_request_inner(connection, request.clone(), &ctx_with_request).await;

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
    conn: &dyn ServerConn,
    request: JSONRPCRequest,
    ctx: &ServerCtx,
) -> Result<serde_json::Value> {
    let mut request_obj = serde_json::Map::new();
    request_obj.insert(
        "method".to_string(),
        serde_json::Value::String(request.request.method.clone()),
    );
    if let Some(params) = request.request.params {
        for (key, value) in params.other {
            request_obj.insert(key, value);
        }
    }

    let client_request =
        match serde_json::from_value::<ClientRequest>(serde_json::Value::Object(request_obj)) {
            Ok(req) => req,
            Err(err) => {
                // Check if it's an unknown method or invalid parameters
                let err_str = err.to_string();
                if err_str.contains("unknown variant") {
                    return Err(Error::MethodNotFound(request.request.method.clone()));
                } else {
                    // It's a known method with invalid parameters
                    return Err(Error::InvalidParams(format!(
                        "Invalid parameters for {}: {}",
                        request.request.method, err
                    )));
                }
            }
        };

    match client_request {
        ClientRequest::Initialize {
            protocol_version,
            capabilities,
            client_info,
        } => conn
            .initialize(ctx, protocol_version, capabilities, client_info)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        ClientRequest::Ping => {
            info!("Server received ping request, sending automatic response");
            conn.pong(ctx).await.map(|_| serde_json::json!({}))
        }
        ClientRequest::ListTools { cursor } => conn
            .list_tools(ctx, cursor)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        ClientRequest::CallTool { name, arguments } => conn
            .call_tool(ctx, name, arguments)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        ClientRequest::ListResources { cursor } => conn
            .list_resources(ctx, cursor)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        ClientRequest::ListResourceTemplates { cursor } => conn
            .list_resource_templates(ctx, cursor)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        ClientRequest::ReadResource { uri } => conn
            .read_resource(ctx, uri)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        ClientRequest::Subscribe { uri } => conn
            .resources_subscribe(ctx, uri)
            .await
            .map(|_| serde_json::json!({})),
        ClientRequest::Unsubscribe { uri } => conn
            .resources_unsubscribe(ctx, uri)
            .await
            .map(|_| serde_json::json!({})),
        ClientRequest::ListPrompts { cursor } => conn
            .list_prompts(ctx, cursor)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        ClientRequest::GetPrompt { name, arguments } => conn
            .get_prompt(ctx, name, arguments)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        ClientRequest::Complete {
            reference,
            argument,
            context: _,
        } => conn
            .complete(ctx, reference, argument)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        ClientRequest::SetLevel { level } => conn
            .set_level(ctx, level)
            .await
            .map(|_| serde_json::json!({})),
    }
}

/// Handle a notification using the Connection trait
async fn handle_notification(
    connection: &dyn ServerConn,
    notification: JSONRPCNotification,
    context: &ServerCtx,
) -> Result<()> {
    debug!(
        "Received notification: {}",
        notification.notification.method
    );

    // Build a value that matches the shape expected by ClientNotification.
    let mut object = serde_json::Map::new();
    object.insert(
        "method".to_string(),
        serde_json::Value::String(notification.notification.method.clone()),
    );

    if let Some(params) = notification.notification.params {
        for (k, v) in params.other {
            object.insert(k, v);
        }
    }

    let value = serde_json::Value::Object(object);

    match serde_json::from_value::<ClientNotification>(value) {
        Ok(typed) => connection.notification(context, typed).await,
        Err(e) => {
            warn!("Failed to deserialize client notification: {}", e);
            Ok(())
        }
    }
}
