use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, error, info, warn};

use crate::{
    client_connection::{ClientConn, ClientCtx},
    connection::{create_empty_response, result_to_jsonrpc_response},
    error::{Error, Result},
    schema::*,
    transport::{
        GenericDuplex, StdioTransport, StreamTransport, TcpClientTransport, Transport,
        TransportStream,
    },
};
use async_trait::async_trait;

/// Type for handling either a response or error from JSON-RPC
#[derive(Debug)]
enum ResponseOrError {
    Response(JSONRPCResponse),
    Error(JSONRPCError),
}

type TransportSink = Arc<Mutex<SplitSink<Box<dyn TransportStream>, JSONRPCMessage>>>;

/// Default no-op implementation of ClientConn for unit type
#[async_trait]
impl ClientConn for () {
    // All methods use default implementations
}

/// MCP Client implementation
pub struct Client<C = ()>
where
    C: ClientConn + Send,
{
    transport_tx: Option<TransportSink>,
    pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<ResponseOrError>>>>,
    notification_tx: mpsc::Sender<JSONRPCNotification>,
    notification_rx: Option<mpsc::Receiver<JSONRPCNotification>>,
    next_request_id: Arc<Mutex<u64>>,
    connection: Option<C>,
    name: String,
    version: String,
    client_capabilities: ClientCapabilities,
}

impl Client<()> {
    /// Create a new MCP client with default configuration
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        let (notification_tx, notification_rx) = mpsc::channel(100);
        Self {
            transport_tx: None,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            notification_tx,
            notification_rx: Some(notification_rx),
            next_request_id: Arc::new(Mutex::new(1)),
            connection: None,
            name: name.into(),
            version: version.into(),
            client_capabilities: ClientCapabilities::default(),
        }
    }

    /// Set the client connection handler
    pub fn new_with_connection<C: ClientConn>(
        name: impl Into<String>,
        version: impl Into<String>,
        connection: C,
    ) -> Client<C> {
        let (notification_tx, notification_rx) = mpsc::channel(100);
        Client {
            transport_tx: None,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            notification_tx,
            notification_rx: Some(notification_rx),
            next_request_id: Arc::new(Mutex::new(1)),
            connection: Some(connection),
            name: name.into(),
            version: version.into(),
            client_capabilities: ClientCapabilities::default(),
        }
    }

    /// Set the client capabilities
    pub fn with_capabilities(mut self, capabilities: ClientCapabilities) -> Self {
        self.client_capabilities = capabilities;
        self
    }
}

impl<C> Client<C>
where
    C: ClientConn + Send + 'static,
{
    /// Connect using the provided transport
    pub async fn connect(&mut self, mut transport: Box<dyn Transport>) -> Result<()> {
        transport.connect().await?;
        let stream = transport.framed()?;

        // Start the message handler task before storing transport
        self.start_message_handler(stream).await?;

        info!("MCP client connected");
        Ok(())
    }

    /// Initialize the connection with the server
    pub async fn initialize(&mut self) -> Result<InitializeResult> {
        let client_info = Implementation {
            name: self.name.clone(),
            version: self.version.clone(),
        };

        let request = ClientRequest::Initialize {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            capabilities: self.client_capabilities.clone(),
            client_info,
        };

        let result: InitializeResult = self.request(request).await?;

        // Send the initialized notification to complete the handshake
        self.send_notification("notifications/initialized", None)
            .await?;

        Ok(result)
    }

    /// List available tools from the server
    pub async fn list_tools(&mut self) -> Result<ListToolsResult> {
        self.request(ClientRequest::ListTools { cursor: None })
            .await
    }

    /// List available tools from the server with pagination cursor
    pub async fn list_tools_with_cursor(
        &mut self,
        cursor: impl Into<Cursor>,
    ) -> Result<ListToolsResult> {
        self.request(ClientRequest::ListTools {
            cursor: Some(cursor.into()),
        })
        .await
    }

    /// List available resources from the server
    pub async fn list_resources(&mut self) -> Result<ListResourcesResult> {
        self.request(ClientRequest::ListResources { cursor: None })
            .await
    }

    /// List available resources from the server with pagination cursor
    pub async fn list_resources_with_cursor(
        &mut self,
        cursor: impl Into<Cursor>,
    ) -> Result<ListResourcesResult> {
        self.request(ClientRequest::ListResources {
            cursor: Some(cursor.into()),
        })
        .await
    }

    /// List available resource templates from the server
    pub async fn list_resource_templates(&mut self) -> Result<ListResourceTemplatesResult> {
        self.request(ClientRequest::ListResourceTemplates { cursor: None })
            .await
    }

    /// List available resource templates from the server with pagination cursor
    pub async fn list_resource_templates_with_cursor(
        &mut self,
        cursor: impl Into<Cursor>,
    ) -> Result<ListResourceTemplatesResult> {
        self.request(ClientRequest::ListResourceTemplates {
            cursor: Some(cursor.into()),
        })
        .await
    }

    /// List available prompts from the server
    pub async fn list_prompts(&mut self) -> Result<ListPromptsResult> {
        self.request(ClientRequest::ListPrompts { cursor: None })
            .await
    }

    /// List available prompts from the server with pagination cursor
    pub async fn list_prompts_with_cursor(
        &mut self,
        cursor: impl Into<Cursor>,
    ) -> Result<ListPromptsResult> {
        self.request(ClientRequest::ListPrompts {
            cursor: Some(cursor.into()),
        })
        .await
    }

    /// Call a tool on the server without arguments
    ///
    /// This is a convenience method for calling tools that don't require arguments.
    pub async fn call_tool_without_args(
        &mut self,
        name: impl Into<String>,
    ) -> Result<CallToolResult> {
        let request = ClientRequest::CallTool {
            name: name.into(),
            arguments: None,
        };
        self.request(request).await
    }

    /// Call a tool on the server with arguments
    pub async fn call_tool<T>(
        &mut self,
        name: impl Into<String>,
        arguments: &T,
    ) -> Result<CallToolResult>
    where
        T: serde::Serialize,
    {
        let value = serde_json::to_value(arguments)?;
        let arguments = if let serde_json::Value::Object(map) = value {
            Some(map.into_iter().collect())
        } else {
            Some(std::collections::HashMap::new())
        };

        let request = ClientRequest::CallTool {
            name: name.into(),
            arguments,
        };
        self.request(request).await
    }

    /// Send a ping to the server
    pub async fn ping(&mut self) -> Result<()> {
        let _: EmptyResult = self.request(ClientRequest::Ping).await?;
        Ok(())
    }

    /// Take the notification receiver channel
    pub fn take_notification_receiver(&mut self) -> Option<mpsc::Receiver<JSONRPCNotification>> {
        self.notification_rx.take()
    }

    /// Connect to a TCP server and initialize the connection
    ///
    /// This is a convenience method that creates a TCP transport,
    /// connects to the server, and performs the initialization handshake.
    pub async fn connect_tcp(&mut self, addr: impl Into<String>) -> Result<InitializeResult> {
        let transport = Box::new(TcpClientTransport::new(addr));
        self.connect(transport).await?;
        self.initialize().await
    }

    /// Connect via stdio and initialize the connection
    ///
    /// This is a convenience method that creates a stdio transport,
    /// connects to the server, and performs the initialization handshake.
    pub async fn connect_stdio(&mut self) -> Result<InitializeResult> {
        let transport = Box::new(StdioTransport::new());
        self.connect(transport).await?;
        self.initialize().await
    }

    /// Connect using generic AsyncRead and AsyncWrite streams
    ///
    /// This method allows you to connect to a server using any pair of
    /// AsyncRead and AsyncWrite streams, such as process stdio, pipes,
    /// or custom implementations.
    pub async fn connect_stream<R, W>(&mut self, reader: R, writer: W) -> Result<()>
    where
        R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
        W: tokio::io::AsyncWrite + Send + Sync + Unpin + 'static,
    {
        let duplex = GenericDuplex::new(reader, writer);
        let transport = Box::new(StreamTransport::new(duplex));
        self.connect(transport).await
    }

    /// Spawn a process and connect to it via its stdin/stdout
    ///
    /// This method spawns a new process and establishes an MCP connection
    /// through its standard input and output streams.
    ///
    /// # Arguments
    /// * `command` - A configured tokio::process::Command ready to be spawned
    ///
    /// # Returns
    /// Returns the spawned Child process handle, allowing you to manage the
    /// process lifecycle (e.g., wait for completion, kill it, etc.)
    pub async fn connect_process(&mut self, mut command: Command) -> Result<Child> {
        // Configure the command to use piped stdio
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()); // Let stderr pass through for debugging

        // Spawn the process
        let mut child = command
            .spawn()
            .map_err(|e| Error::Transport(format!("Failed to spawn process: {e}")))?;

        // Take ownership of stdin and stdout
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Transport("Failed to capture process stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Transport("Failed to capture process stdout".to_string()))?;

        // Connect using the process streams
        // Note: We swap the order here - the child's stdout is our reader,
        // and the child's stdin is our writer
        self.connect_stream(stdout, stdin).await?;

        Ok(child)
    }

    /// Send a request and wait for response
    async fn request<T>(&mut self, request: ClientRequest) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let id = self.next_request_id().await;
        let (tx, rx) = oneshot::channel();

        // Store the response channel
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id.clone(), tx);
        }

        // Create the JSON-RPC request
        let jsonrpc_request = JSONRPCRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: RequestId::String(id.clone()),
            request: Request {
                method: request.method().to_string(),
                params: Some(RequestParams {
                    meta: None,
                    other: serde_json::to_value(&request)?
                        .as_object()
                        .unwrap_or(&serde_json::Map::new())
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                }),
            },
        };

        self.send_message(JSONRPCMessage::Request(jsonrpc_request))
            .await?;

        // Wait for response with timeout
        match rx.await {
            Ok(response_or_error) => {
                match response_or_error {
                    ResponseOrError::Response(response) => {
                        // Create a combined Value from the result's fields
                        let mut result_value = serde_json::Map::new();

                        // Add metadata if present
                        if let Some(meta) = response.result.meta {
                            result_value.insert("_meta".to_string(), serde_json::to_value(meta)?);
                        }

                        // Add all other fields
                        for (key, value) in response.result.other {
                            result_value.insert(key, value);
                        }

                        // Deserialize directly from the combined map
                        serde_json::from_value(serde_json::Value::Object(result_value)).map_err(
                            |e| Error::Protocol(format!("Failed to deserialize response: {e}")),
                        )
                    }
                    ResponseOrError::Error(error) => {
                        // Map JSON-RPC errors to appropriate MCPError variants
                        match error.error.code {
                            METHOD_NOT_FOUND => Err(Error::MethodNotFound(error.error.message)),
                            INVALID_PARAMS => Err(Error::InvalidParams(format!(
                                "{}: {}",
                                request.method(),
                                error.error.message
                            ))),
                            _ => Err(Error::Protocol(format!(
                                "JSON-RPC error {}: {}",
                                error.error.code, error.error.message
                            ))),
                        }
                    }
                }
            }
            Err(e) => {
                error!("Response channel closed for request {}: {}", id, e);
                // Remove the pending request
                self.pending_requests.lock().await.remove(&id);
                Err(Error::Protocol("Response channel closed".to_string()))
            }
        }
    }

    /// Send a message through the transport
    async fn send_message(&mut self, message: JSONRPCMessage) -> Result<()> {
        if let Some(transport_tx) = &self.transport_tx {
            let mut tx = transport_tx.lock().await;
            tx.send(message).await?;
            Ok(())
        } else {
            Err(Error::Transport("Not connected".to_string()))
        }
    }

    /// Send a notification to the server
    async fn send_notification(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<()> {
        let notification_params = params.map(|v| NotificationParams {
            meta: None,
            other: if let Some(obj) = v.as_object() {
                obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            } else {
                HashMap::new()
            },
        });

        let notification = JSONRPCNotification {
            jsonrpc: JSONRPC_VERSION.to_string(),
            notification: Notification {
                method: method.to_string(),
                params: notification_params,
            },
        };

        self.send_message(JSONRPCMessage::Notification(notification))
            .await
    }

    /// Generate the next request ID
    async fn next_request_id(&self) -> String {
        let mut id = self.next_request_id.lock().await;
        let current = *id;
        *id += 1;
        format!("req-{current}")
    }

    /// Start the background task that handles incoming messages
    async fn start_message_handler(&mut self, stream: Box<dyn TransportStream>) -> Result<()> {
        let pending_requests = self.pending_requests.clone();
        let notification_tx = self.notification_tx.clone();

        // Split the transport stream into read and write halves
        let (tx, mut rx) = stream.split();

        // Wrap the sink in an Arc<Mutex> for sharing
        let tx = Arc::new(Mutex::new(tx));

        // Store the sender half for sending messages
        self.transport_tx = Some(tx.clone());

        // Take the connection if present
        let mut connection = self.connection.take();

        // Create broadcast channel for client notifications
        let (client_notification_tx, mut client_notification_rx) =
            tokio::sync::broadcast::channel(100);

        // Create the context for the connection
        let context = ClientCtx::new(client_notification_tx.clone());

        // Initialize connection if present
        if let Some(conn) = &mut connection {
            conn.on_connect(context.clone()).await?;
        }

        // Clone sink for notification handler
        let notification_sink = tx.clone();

        // Spawn a task to handle incoming messages
        tokio::spawn(async move {
            debug!("Message handler started");

            loop {
                tokio::select! {
                    // Handle incoming messages from server
                    result = rx.next() => {
                        match result {
                            Some(Ok(message)) => {
                                debug!("Received message: {:?}", message);

                                match message {
                                    JSONRPCMessage::Response(response) => {
                                        // Extract the ID and find the corresponding request
                                        if let RequestId::String(id) = &response.id {
                                            let mut pending = pending_requests.lock().await;
                                            if let Some(tx) = pending.remove(id) {
                                                // Send the response to the waiting request
                                                let _ = tx.send(ResponseOrError::Response(response));
                                            } else {
                                                warn!("Received response for unknown request ID: {}", id);
                                            }
                                        }
                                    }
                                    JSONRPCMessage::Notification(notification) => {
                                        // Attempt to convert the JSON-RPC notification into a typed
                                        // ServerNotification so that custom connection handlers can
                                        // react to it.
                                        if let Some(conn) = &mut connection {
                                            if let Err(err) = handle_server_notification(conn, context.clone(), notification.clone()).await {
                                                error!("Failed to handle server notification: {}", err);
                                            }
                                        }

                                        // Always forward raw notifications to the public channel so
                                        // that users that are not using a ClientConnection can still
                                        // receive them.
                                        if let Err(e) = notification_tx.send(notification).await {
                                            error!("Failed to send notification: {}", e);
                                            break;
                                        }
                                    }
                                    JSONRPCMessage::Error(error) => {
                                        // Handle JSON-RPC errors
                                        if let RequestId::String(id) = &error.id {
                                            let mut pending = pending_requests.lock().await;
                                            if let Some(tx) = pending.remove(id) {
                                                let _ = tx.send(ResponseOrError::Error(error));
                                            } else {
                                                warn!("Received error for unknown request ID: {}", id);
                                            }
                                        } else {
                                            error!(
                                                "Received error with non-string request ID: {:?}",
                                                error.id
                                            );
                                        }
                                    }
                                    JSONRPCMessage::Request(request) => {
                                        if let Some(conn) = &mut connection {
                                            let response = handle_server_request(conn, context.clone(), request).await;
                                            let mut sink = tx.lock().await;
                                            if let Err(e) = sink.send(response).await {
                                                error!("Failed to send response to server: {}", e);
                                                break;
                                            }
                                        } else {
                                            // No connection handler, handle built-in requests only
                                            if request.request.method == "ping" {
                                                info!("Received ping request from server, sending response");
                                                let response = create_empty_response(request.id);
                                                let mut sink = tx.lock().await;
                                                if let Err(e) = sink.send(JSONRPCMessage::Response(response)).await {
                                                    error!("Failed to send ping response: {}", e);
                                                    break;
                                                }
                                            } else {
                                                warn!(
                                                    "Received request from server but no connection handler: {}",
                                                    request.request.method
                                                );
                                            }
                                        }
                                    }
                                    JSONRPCMessage::BatchRequest(_batch) => {
                                        // Clients typically don't receive batch requests from servers
                                        warn!("Received unexpected batch request from server");
                                    }
                                    JSONRPCMessage::BatchResponse(_batch) => {
                                        // TODO: Handle batch responses if we implement batch requests
                                        warn!(
                                            "Received batch response - batch requests not yet implemented"
                                        );
                                    }
                                }
                            }
                            Some(Err(e)) => {
                                error!("Error receiving message: {}", e);
                                break;
                            }
                            None => {
                                info!("Server disconnected");
                                break;
                            }
                        }
                    }

                    // Forward client notifications to server
                    result = client_notification_rx.recv() => {
                        match result {
                            Ok(notification) => {
                                let jsonrpc_notification = crate::connection::create_jsonrpc_notification(notification);
                                let mut sink = notification_sink.lock().await;
                                if let Err(e) = sink.send(JSONRPCMessage::Notification(jsonrpc_notification)).await {
                                    error!("Error sending notification to server: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                debug!("Client notification channel closed: {}", e);
                                // This is expected when the client shuts down
                            }
                        }
                    }
                }
            }

            // Clean up connection
            if let Some(mut conn) = connection {
                if let Err(e) = conn.on_disconnect(context).await {
                    error!("Error during connection disconnect: {}", e);
                }
            }

            info!("Message handler stopped");
        });

        Ok(())
    }
}

/// Handle a request from the server using the ClientConnection trait
async fn handle_server_request<C: ClientConn>(
    connection: &mut C,
    context: ClientCtx,
    request: JSONRPCRequest,
) -> JSONRPCMessage {
    let result = handle_server_request_inner(connection, context, request.clone()).await;
    result_to_jsonrpc_response(request.id, result)
}

/// Inner handler that returns Result<serde_json::Value>
async fn handle_server_request_inner<C: ClientConn>(
    connection: &mut C,
    context: ClientCtx,
    request: JSONRPCRequest,
) -> Result<serde_json::Value> {
    // Convert RequestParams to serde_json::Value
    let params = request
        .request
        .params
        .map(|p| serde_json::to_value(p.other))
        .transpose()?;

    match request.request.method.as_str() {
        "ping" => {
            info!("Server sent ping request, sending pong");
            connection
                .pong(context)
                .await
                .map(|_| serde_json::json!({}))
        }
        "sampling/createMessage" => {
            let p = params.ok_or_else(|| {
                Error::InvalidParams(
                    "sampling/createMessage: Missing required parameters".to_string(),
                )
            })?;
            let params = serde_json::from_value::<CreateMessageParams>(p)
                .map_err(|e| Error::InvalidParams(format!("sampling/createMessage: {e}")))?;
            connection
                .create_message(context, &request.request.method, params)
                .await
                .and_then(|result| serde_json::to_value(result).map_err(Into::into))
        }
        "roots/list" => connection
            .list_roots(context)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        method => Err(Error::MethodNotFound(format!(
            "Unknown server method: {method}"
        ))),
    }
}

/// Convert a JSON-RPC notification into a typed ServerNotification and pass it
/// to the connection implementation for further handling.
async fn handle_server_notification<C: ClientConn>(
    connection: &mut C,
    context: ClientCtx,
    notification: JSONRPCNotification,
) -> Result<()> {
    // Build a serde_json::Value representing the notification in the shape
    // expected by the ServerNotification enum (which is `{"method": "...", ...}`).
    use serde_json::Value;

    let mut obj = serde_json::Map::new();
    obj.insert(
        "method".to_string(),
        Value::String(notification.notification.method.clone()),
    );

    if let Some(params) = notification.notification.params {
        for (k, v) in params.other {
            obj.insert(k, v);
        }
    }

    let value = Value::Object(obj);

    match serde_json::from_value::<ClientNotification>(value) {
        Ok(typed) => connection.notification(context, typed).await,
        Err(e) => Err(Error::InvalidParams(format!(
            "Failed to parse server notification: {e}",
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagination_api() {
        // This test just verifies the API is ergonomic - it doesn't run async code
        let mut client = Client::new("test-client", "1.0.0");

        // These should all compile cleanly
        std::mem::drop(async {
            // Simple calls without cursors
            client.list_tools().await.unwrap();
            client.list_resources().await.unwrap();
            client.list_prompts().await.unwrap();
            client.list_resource_templates().await.unwrap();

            // Calls with cursor as &str
            client.list_tools_with_cursor("cursor").await.unwrap();
            client.list_resources_with_cursor("cursor").await.unwrap();
            client.list_prompts_with_cursor("cursor").await.unwrap();
            client
                .list_resource_templates_with_cursor("cursor")
                .await
                .unwrap();

            // Calls with cursor as String
            let cursor = "next_page".to_string();
            client.list_tools_with_cursor(cursor.clone()).await.unwrap();
            client
                .list_resources_with_cursor(cursor.clone())
                .await
                .unwrap();
            client
                .list_prompts_with_cursor(cursor.clone())
                .await
                .unwrap();
            client
                .list_resource_templates_with_cursor(cursor)
                .await
                .unwrap();
        });
    }

    #[test]
    fn test_call_tool_api() {
        // This test just verifies the API is ergonomic - it doesn't run async code
        let mut client = Client::new("test-client", "1.0.0");

        // These should all compile cleanly
        std::mem::drop(async {
            // Call without arguments
            client.call_tool_without_args("my_tool").await.unwrap();

            // Call with String for tool name
            let tool_name = "another_tool".to_string();
            client.call_tool_without_args(tool_name).await.unwrap();

            // Call with &String
            let tool_name = "third_tool".to_string();
            client.call_tool_without_args(&tool_name).await.unwrap();

            // Call with serde_json::Value arguments
            let args = serde_json::json!({"param": "value"});
            client.call_tool("tool_with_args", &args).await.unwrap();

            // Call with struct that implements Serialize
            #[derive(serde::Serialize)]
            struct MyArgs {
                param: String,
                count: i32,
            }
            let my_args = MyArgs {
                param: "value".to_string(),
                count: 42,
            };
            client
                .call_tool("tool_with_struct", &my_args)
                .await
                .unwrap();

            // Call with HashMap
            let mut map = std::collections::HashMap::new();
            map.insert("key", "value");
            client.call_tool("tool_with_map", &map).await.unwrap();
        });
    }

    use crate::server::{Server, ServerHandle};
    use crate::transport::TestTransport;

    async fn setup_client_server() -> (Client, ServerHandle) {
        let (client_transport, server_transport) = TestTransport::create_pair();

        // Create a minimal test connection
        #[derive(Debug, Default)]
        struct TestConnection;

        #[async_trait::async_trait]
        impl crate::server_connection::ServerConn for TestConnection {
            async fn initialize(
                &mut self,
                _context: crate::server_connection::ServerCtx,
                _protocol_version: String,
                _capabilities: ClientCapabilities,
                _client_info: Implementation,
            ) -> Result<InitializeResult> {
                Ok(InitializeResult::new("test-server", "1.0.0"))
            }
        }

        let server = Server::default().with_connection(TestConnection::default);
        let server_handle = ServerHandle::new(server, server_transport)
            .await
            .expect("Failed to start server");

        let mut client = Client::new("test-client", "1.0.0");
        client
            .connect(client_transport)
            .await
            .expect("Failed to connect");

        client.initialize().await.expect("Failed to initialize");

        (client, server_handle)
    }

    // Test that a ClientConnection implementation receives notifications sent
    // by the server.
    #[tokio::test]
    async fn test_client_receives_server_notification() {
        use tokio::sync::oneshot;

        // Channel to signal when notification is received
        let (tx_notif, rx_notif) = oneshot::channel::<()>();

        // Custom client connection that records notifications
        struct NotifClientConnection {
            tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
        }

        impl Clone for NotifClientConnection {
            fn clone(&self) -> Self {
                Self {
                    tx: self.tx.clone(),
                }
            }
        }

        #[async_trait::async_trait]
        impl crate::client_connection::ClientConn for NotifClientConnection {
            async fn notification(
                &mut self,
                _context: crate::client_connection::ClientCtx,
                notification: crate::schema::ClientNotification,
            ) -> Result<()> {
                if matches!(
                    notification,
                    crate::schema::ClientNotification::RootsListChanged
                ) {
                    let mut tx_guard = self.tx.lock().await;
                    if let Some(tx) = tx_guard.take() {
                        let _ = tx.send(());
                    }
                }
                Ok(())
            }
        }

        // Minimal server connection
        #[derive(Debug, Default)]
        struct DummyServerConnection;

        #[async_trait::async_trait]
        impl crate::server_connection::ServerConn for DummyServerConnection {
            async fn initialize(
                &mut self,
                _context: crate::server_connection::ServerCtx,
                _protocol_version: String,
                _capabilities: ClientCapabilities,
                _client_info: Implementation,
            ) -> Result<InitializeResult> {
                Ok(InitializeResult::new("test-server", "1.0.0"))
            }
        }

        // Create transport pair
        let (client_transport, server_transport) = TestTransport::create_pair();

        // Start server
        let server = Server::default().with_connection(DummyServerConnection::default);
        let server_handle = ServerHandle::new(server, server_transport)
            .await
            .expect("Failed to start server");

        // Create client with notif connection
        let mut client = Client::new_with_connection(
            "test-client",
            "1.0.0",
            NotifClientConnection {
                tx: Arc::new(Mutex::new(Some(tx_notif))),
            },
        );

        // Connect and initialize
        client
            .connect(client_transport)
            .await
            .expect("Failed to connect");

        client.initialize().await.expect("Failed to initialize");

        // Send server notification
        server_handle.send_server_notification(crate::schema::ClientNotification::RootsListChanged);

        // Wait for notification to be received
        tokio::time::timeout(std::time::Duration::from_secs(1), rx_notif)
            .await
            .expect("Notification not received")
            .expect("Receiver dropped");
    }

    // Test that a ServerConnection implementation receives notifications sent
    // by the client.
    #[tokio::test]
    async fn test_server_receives_client_notification() {
        use tokio::sync::oneshot;

        // Channel to notify when server receives notification
        let (tx_notif, rx_notif) = oneshot::channel();

        use std::sync::{Arc, Mutex as StdMutex};

        // Server connection that records notification
        #[derive(Debug, Default)]
        struct NotifServerConnection {
            tx: Arc<StdMutex<Option<oneshot::Sender<()>>>>,
        }

        #[async_trait::async_trait]
        impl crate::server_connection::ServerConn for NotifServerConnection {
            async fn initialize(
                &mut self,
                _context: crate::server_connection::ServerCtx,
                _protocol_version: String,
                _capabilities: ClientCapabilities,
                _client_info: Implementation,
            ) -> Result<InitializeResult> {
                Ok(InitializeResult::new("test-server", "1.0.0"))
            }

            async fn notification(
                &mut self,
                _context: crate::server_connection::ServerCtx,
                notification: crate::schema::ServerNotification,
            ) -> Result<()> {
                if matches!(
                    notification,
                    crate::schema::ServerNotification::ToolListChanged
                ) {
                    let maybe_tx = self.tx.lock().unwrap().take();
                    if let Some(tx) = maybe_tx {
                        let _ = tx.send(());
                    }
                }
                Ok(())
            }
        }

        // Client connection that sends a notification on connect
        #[derive(Clone)]
        struct NotifierClientConnection;

        #[async_trait::async_trait]
        impl crate::client_connection::ClientConn for NotifierClientConnection {
            async fn on_connect(
                &mut self,
                context: crate::client_connection::ClientCtx,
            ) -> Result<()> {
                context.send_notification(crate::schema::ServerNotification::ToolListChanged)?;
                Ok(())
            }
        }

        // Create transport pair
        let (client_transport, server_transport) = TestTransport::create_pair();

        let shared_tx: Arc<StdMutex<Option<oneshot::Sender<()>>>> =
            Arc::new(StdMutex::new(Some(tx_notif)));

        // Start server with notif connection
        let server = {
            let tx_clone = shared_tx.clone();
            Server::default().with_connection(move || NotifServerConnection {
                tx: tx_clone.clone(),
            })
        };
        let server_handle = ServerHandle::new(server, server_transport)
            .await
            .expect("Failed to start server");

        // Create client with notifier connection
        let mut client =
            Client::new_with_connection("test-client", "1.0.0", NotifierClientConnection);

        // Connect and initialize
        client
            .connect(client_transport)
            .await
            .expect("Failed to connect");

        client.initialize().await.expect("Failed to initialize");

        // Wait for server to receive notification
        tokio::time::timeout(std::time::Duration::from_secs(1), rx_notif)
            .await
            .expect("Server did not receive notification")
            .expect("Receiver dropped");
        std::mem::drop(server_handle);
    }

    #[test]
    fn test_client_creation() {
        let client = Client::new("test-client", "1.0.0");
        assert!(client.transport_tx.is_none());
    }

    #[tokio::test]
    async fn test_next_request_id() {
        let client = Client::new("test-client", "1.0.0");
        assert_eq!(client.next_request_id().await, "req-1");
        assert_eq!(client.next_request_id().await, "req-2");
    }

    #[tokio::test]
    async fn test_client_ping_server() {
        let (mut client, _server) = setup_client_server().await;
        client.ping().await.expect("Ping failed");
    }

    #[tokio::test]
    async fn test_multiple_client_pings() {
        let (mut client, _server) = setup_client_server().await;
        for i in 0..20 {
            client
                .ping()
                .await
                .unwrap_or_else(|_| panic!("Ping {i} failed"));
        }
    }

    #[tokio::test]
    async fn test_ping_performance() {
        let (mut client, _server) = setup_client_server().await;

        let start = std::time::Instant::now();
        let num_pings = 50;
        for _ in 0..num_pings {
            client.ping().await.expect("Ping failed");
        }

        let duration = start.elapsed();
        let pings_per_second = num_pings as f64 / duration.as_secs_f64();
        println!(
            "Client->Server: {num_pings} pings in {duration:?} ({pings_per_second:.1} pings/sec)"
        );
        assert!(
            pings_per_second > 50.0,
            "Too slow: {pings_per_second:.1} pings/sec"
        );
    }

    #[tokio::test]
    async fn test_connect_stream() {
        // Create a pair of duplex streams for testing
        let (client_reader, server_writer) = tokio::io::duplex(8192);
        let (server_reader, client_writer) = tokio::io::duplex(8192);

        // Create a minimal test connection
        #[derive(Debug, Default)]
        struct TestStreamConnection;

        #[async_trait::async_trait]
        impl crate::server_connection::ServerConn for TestStreamConnection {
            async fn initialize(
                &mut self,
                _context: crate::server_connection::ServerCtx,
                _protocol_version: String,
                _capabilities: ClientCapabilities,
                _client_info: Implementation,
            ) -> Result<InitializeResult> {
                Ok(InitializeResult::new("test-server", "1.0.0"))
            }
        }

        // Create and configure server
        let server = Server::default()
            .with_connection(TestStreamConnection::default)
            .with_capabilities(ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(true),
                }),
                ..Default::default()
            });

        // Create server transport from the streams
        let server_duplex = crate::transport::GenericDuplex::new(server_reader, server_writer);
        let server_transport = Box::new(crate::transport::StreamTransport::new(server_duplex));

        let _server_handle = ServerHandle::new(server, server_transport)
            .await
            .expect("Failed to start server");

        // Create and connect client using connect_stream
        let mut client = Client::new("test-client", "1.0.0");
        client
            .connect_stream(client_reader, client_writer)
            .await
            .expect("Failed to connect client");

        // Test that we can initialize
        let result = client.initialize().await.expect("Failed to initialize");

        assert_eq!(result.server_info.name, "test-server");

        // Test that we can ping
        client.ping().await.expect("Ping failed");
    }

    #[tokio::test]
    async fn test_connect_process() {
        // This test would require an actual MCP server binary to spawn
        // For now, we'll just test that the API compiles and handles errors correctly

        let mut client = Client::new("test-client", "1.0.0");

        // Try to spawn a non-existent process
        let cmd = tokio::process::Command::new("non-existent-mcp-server");
        let result = client.connect_process(cmd).await;

        // Should fail with a transport error
        assert!(result.is_err());
        if let Err(Error::Transport(msg)) = result {
            assert!(msg.contains("Failed to spawn process"));
        } else {
            panic!("Expected Transport error");
        }
    }
}
