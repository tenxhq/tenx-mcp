use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{debug, error, info, warn};

use crate::api::ServerAPI;
use crate::{
    client_connection::{ClientConn, ClientCtx},
    connection::result_to_jsonrpc_response,
    error::{Error, Result},
    schema::*,
    transport::{GenericDuplex, StdioTransport, StreamTransport, TcpClientTransport, Transport},
};
use async_trait::async_trait;

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
    request_handler: crate::request_handler::RequestHandler,
    connection: C,
    name: String,
    version: String,
    client_capabilities: ClientCapabilities,
}

impl Client<()> {
    /// Create a new MCP client with default configuration
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            request_handler: crate::request_handler::RequestHandler::new(None, "req".to_string()),
            connection: (),
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
        Client {
            request_handler: crate::request_handler::RequestHandler::new(None, "req".to_string()),
            connection,
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
    pub(crate) async fn connect(&mut self, mut transport: Box<dyn Transport>) -> Result<()> {
        transport.connect().await?;
        let stream = transport.framed()?;

        // Start the message handler task before storing transport
        self.start_message_handler(stream).await?;

        info!("MCP client connected");
        Ok(())
    }

    /// Initialize the connection with the server
    ///
    /// This is a convenience method that uses the client's configured name, version,
    /// and capabilities with the latest protocol version.
    pub async fn init(&mut self) -> Result<InitializeResult> {
        let client_info = Implementation {
            name: self.name.clone(),
            version: self.version.clone(),
        };

        <Self as ServerAPI>::initialize(
            self,
            LATEST_PROTOCOL_VERSION.to_string(),
            self.client_capabilities.clone(),
            client_info,
        )
        .await
    }

    /// Connect to a TCP server and initialize the connection
    ///
    /// This is a convenience method that creates a TCP transport,
    /// connects to the server, and performs the initialization handshake.
    pub async fn connect_tcp(&mut self, addr: impl Into<String>) -> Result<InitializeResult> {
        let transport = Box::new(TcpClientTransport::new(addr));
        self.connect(transport).await?;
        self.init().await
    }

    /// Connect via stdio and initialize the connection
    ///
    /// This is a convenience method that creates a stdio transport,
    /// connects to the server, and performs the initialization handshake.
    pub async fn connect_stdio(&mut self) -> Result<InitializeResult> {
        let transport = Box::new(StdioTransport::new());
        self.connect(transport).await?;
        self.init().await
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
        self.request_handler.request(request).await
    }

    /// Send a notification to the server
    async fn send_notification(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<()> {
        self.request_handler.send_notification(method, params).await
    }

    /// Start the background task that handles incoming messages
    async fn start_message_handler(
        &mut self,
        stream: Box<dyn crate::transport::TransportStream>,
    ) -> Result<()> {
        let request_handler = self.request_handler.clone();

        // Split the transport stream into read and write halves
        let (tx, mut rx) = stream.split();

        // Wrap the sink in an Arc<Mutex> for sharing
        let tx = std::sync::Arc::new(tokio::sync::Mutex::new(tx));

        // Store the sender half for sending messages
        self.request_handler.set_transport(tx.clone());

        // Clone the connection for use in the handler
        let connection = self.connection.clone();

        // Create broadcast channel for client notifications
        let (client_notification_tx, mut client_notification_rx) =
            tokio::sync::broadcast::channel(100);

        // Create the context for the connection
        let context = ClientCtx::new(client_notification_tx.clone());

        // Initialize connection
        connection.on_connect(context.clone()).await?;

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
                                        request_handler.handle_response(response).await;
                                    }
                                    JSONRPCMessage::Notification(notification) => {
                                        // Convert the JSON-RPC notification into a typed
                                        // ServerNotification and pass it to the connection handler.
                                        if let Err(err) = handle_server_notification(&connection, context.clone(), notification).await {
                                            error!("Failed to handle server notification: {}", err);
                                        }
                                    }
                                    JSONRPCMessage::Error(error) => {
                                        request_handler.handle_error(error).await;
                                    }
                                    JSONRPCMessage::Request(request) => {
                                        tracing::info!("Client received request from server: {:?}", request.id);
                                        let response = handle_server_request(&connection, context.clone(), request).await;
                                        tracing::info!("Client sending response to server: {:?}", match &response {
                                            JSONRPCMessage::Response(r) => format!("{:?}", r.id),
                                            JSONRPCMessage::Error(e) => format!("{:?}", e.id),
                                            _ => "Unknown".to_string()
                                        });
                                        let mut sink = tx.lock().await;
                                        if let Err(e) = sink.send(response).await {
                                            error!("Failed to send response to server: {}", e);
                                            break;
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
            if let Err(e) = connection.on_disconnect(context).await {
                error!("Error during connection disconnect: {}", e);
            }

            info!("Message handler stopped");
        });

        Ok(())
    }
}

/// Implementation of ServerAPI trait for Client
#[async_trait]
impl<C> ServerAPI for Client<C>
where
    C: ClientConn + Send + Sync + 'static,
{
    /// Initialize the connection with protocol version and capabilities
    async fn initialize(
        &mut self,
        protocol_version: String,
        capabilities: ClientCapabilities,
        client_info: Implementation,
    ) -> Result<InitializeResult> {
        let request = ClientRequest::Initialize {
            protocol_version,
            capabilities,
            client_info,
        };

        let result: InitializeResult = self.request(request).await?;

        // Send the initialized notification to complete the handshake
        self.send_notification("notifications/initialized", None)
            .await?;

        Ok(result)
    }

    /// Respond to ping requests
    async fn ping(&mut self) -> Result<()> {
        let _: EmptyResult = self.request(ClientRequest::Ping).await?;
        Ok(())
    }

    /// List available tools with optional pagination
    async fn list_tools(
        &mut self,
        cursor: impl Into<Option<Cursor>> + Send,
    ) -> Result<ListToolsResult> {
        let cursor: Option<Cursor> = cursor.into();
        self.request(ClientRequest::ListTools { cursor }).await
    }

    /// Call a tool with the given name and arguments
    async fn call_tool(
        &mut self,
        name: impl Into<String> + Send,
        arguments: impl Into<Option<HashMap<String, Value>>> + Send,
    ) -> Result<CallToolResult> {
        let request = ClientRequest::CallTool {
            name: name.into(),
            arguments: arguments.into(),
        };
        self.request(request).await
    }

    /// List available resources with optional pagination
    async fn list_resources(
        &mut self,
        cursor: impl Into<Option<Cursor>> + Send,
    ) -> Result<ListResourcesResult> {
        let cursor: Option<Cursor> = cursor.into();
        self.request(ClientRequest::ListResources { cursor }).await
    }

    /// List resource templates with optional pagination
    async fn list_resource_templates(
        &mut self,
        cursor: impl Into<Option<Cursor>> + Send,
    ) -> Result<ListResourceTemplatesResult> {
        let cursor: Option<Cursor> = cursor.into();
        self.request(ClientRequest::ListResourceTemplates { cursor })
            .await
    }

    /// Read a resource by URI
    async fn resources_read(
        &mut self,
        uri: impl Into<String> + Send,
    ) -> Result<ReadResourceResult> {
        self.request(ClientRequest::ReadResource { uri: uri.into() })
            .await
    }

    /// Subscribe to resource updates
    async fn resources_subscribe(&mut self, uri: impl Into<String> + Send) -> Result<()> {
        let _: EmptyResult = self
            .request(ClientRequest::Subscribe { uri: uri.into() })
            .await?;
        Ok(())
    }

    /// Unsubscribe from resource updates
    async fn resources_unsubscribe(&mut self, uri: impl Into<String> + Send) -> Result<()> {
        let _: EmptyResult = self
            .request(ClientRequest::Unsubscribe { uri: uri.into() })
            .await?;
        Ok(())
    }

    /// List available prompts with optional pagination
    async fn list_prompts(
        &mut self,
        cursor: impl Into<Option<Cursor>> + Send,
    ) -> Result<ListPromptsResult> {
        let cursor: Option<Cursor> = cursor.into();
        self.request(ClientRequest::ListPrompts { cursor }).await
    }

    /// Get a prompt by name with optional arguments
    async fn get_prompt(
        &mut self,
        name: impl Into<String> + Send,
        arguments: Option<HashMap<String, String>>,
    ) -> Result<GetPromptResult> {
        self.request(ClientRequest::GetPrompt {
            name: name.into(),
            arguments,
        })
        .await
    }

    /// Handle completion requests
    async fn complete(
        &mut self,
        reference: Reference,
        argument: ArgumentInfo,
    ) -> Result<CompleteResult> {
        self.request(ClientRequest::Complete {
            reference,
            argument,
        })
        .await
    }

    /// Set the logging level
    async fn set_level(&mut self, level: LoggingLevel) -> Result<()> {
        let _: EmptyResult = self.request(ClientRequest::SetLevel { level }).await?;
        Ok(())
    }
}

/// Handle a request from the server using the ClientConnection trait
async fn handle_server_request<C: ClientConn>(
    connection: &C,
    context: ClientCtx,
    request: JSONRPCRequest,
) -> JSONRPCMessage {
    let result = handle_server_request_inner(connection, context, request.clone()).await;
    result_to_jsonrpc_response(request.id, result)
}

/// Inner handler that returns Result<serde_json::Value>
async fn handle_server_request_inner<C: ClientConn>(
    connection: &C,
    ctx: ClientCtx,
    request: JSONRPCRequest,
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

    let server_request =
        match serde_json::from_value::<ServerRequest>(serde_json::Value::Object(request_obj)) {
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

    match server_request {
        ServerRequest::Ping => {
            info!("Server sent ping request, sending pong");
            connection.pong(ctx).await.map(|_| serde_json::json!({}))
        }
        ServerRequest::CreateMessage(params) => connection
            .create_message(ctx, &request.request.method, *params)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
        ServerRequest::ListRoots => connection
            .list_roots(ctx)
            .await
            .and_then(|result| serde_json::to_value(result).map_err(Into::into)),
    }
}

/// Convert a JSON-RPC notification into a typed ServerNotification and pass it
/// to the connection implementation for further handling.
async fn handle_server_notification<C: ClientConn>(
    connection: &C,
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

    match serde_json::from_value::<ServerNotification>(value) {
        Ok(typed) => connection.notification(context, typed).await,
        Err(e) => Err(Error::InvalidParams(format!(
            "Failed to parse server notification: {e}",
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[test]
    fn test_pagination_api() {
        // This test just verifies the API is ergonomic - it doesn't run async code
        let mut client = Client::new("test-client", "1.0.0");

        // These should all compile cleanly
        std::mem::drop(async {
            // Simple calls without cursors - passing None
            client.list_tools(None).await.unwrap();
            client.list_resources(None).await.unwrap();
            client.list_prompts(None).await.unwrap();
            client.list_resource_templates(None).await.unwrap();

            // Calls with cursor as Cursor type
            let cursor = Cursor::from("cursor");
            client.list_tools(cursor.clone()).await.unwrap();
            client.list_resources(cursor.clone()).await.unwrap();
            client.list_prompts(cursor.clone()).await.unwrap();
            client.list_resource_templates(cursor).await.unwrap();

            // Calls with explicit Some(Cursor)
            client
                .list_tools(Some(Cursor::from("some_cursor")))
                .await
                .unwrap();
            client
                .list_resources(Some(Cursor::from("some_cursor")))
                .await
                .unwrap();
            client
                .list_prompts(Some(Cursor::from("some_cursor")))
                .await
                .unwrap();
            client
                .list_resource_templates(Some(Cursor::from("some_cursor")))
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
            // Call without arguments - passing None
            client.call_tool("my_tool", None).await.unwrap();

            // Call with String for tool name
            let tool_name = "another_tool".to_string();
            client.call_tool(tool_name, None).await.unwrap();

            // Call with &String
            let tool_name = "third_tool".to_string();
            client.call_tool(&tool_name, None).await.unwrap();

            // Call with HashMap arguments directly
            let mut args = std::collections::HashMap::new();
            args.insert("param".to_string(), serde_json::json!("value"));
            client.call_tool("tool_with_args", args).await.unwrap();

            // Call with Some(HashMap)
            let mut args = std::collections::HashMap::new();
            args.insert("key".to_string(), serde_json::json!("value"));
            client.call_tool("tool_with_map", Some(args)).await.unwrap();
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
                &self,
                _context: crate::server::ServerCtx,
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

        client.init().await.expect("Failed to initialize");

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
                &self,
                _context: crate::client_connection::ClientCtx,
                notification: crate::schema::ServerNotification,
            ) -> Result<()> {
                if matches!(
                    notification,
                    crate::schema::ServerNotification::ToolListChanged
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
                &self,
                _context: crate::server::ServerCtx,
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

        client.init().await.expect("Failed to initialize");

        // Send server notification
        server_handle.send_server_notification(crate::schema::ServerNotification::ToolListChanged);

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
                &self,
                _context: crate::server::ServerCtx,
                _protocol_version: String,
                _capabilities: ClientCapabilities,
                _client_info: Implementation,
            ) -> Result<InitializeResult> {
                Ok(InitializeResult::new("test-server", "1.0.0"))
            }

            async fn notification(
                &self,
                _context: crate::server::ServerCtx,
                notification: crate::schema::ClientNotification,
            ) -> Result<()> {
                if matches!(notification, crate::schema::ClientNotification::Initialized) {
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
            async fn on_connect(&self, context: crate::client_connection::ClientCtx) -> Result<()> {
                context.send_notification(crate::schema::ClientNotification::Initialized)?;
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

        client.init().await.expect("Failed to initialize");

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
        assert_eq!(client.name, "test-client");
        assert_eq!(client.version, "1.0.0");
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
                &self,
                _context: crate::server::ServerCtx,
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
        let result = client.init().await.expect("Failed to initialize");

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

    #[tokio::test]
    async fn test_reconnect_works() {
        // Create a minimal test connection
        #[derive(Debug, Default)]
        struct TestConnection;

        #[async_trait::async_trait]
        impl crate::server_connection::ServerConn for TestConnection {
            async fn initialize(
                &self,
                _context: crate::server::ServerCtx,
                _protocol_version: String,
                _capabilities: ClientCapabilities,
                _client_info: Implementation,
            ) -> Result<InitializeResult> {
                Ok(InitializeResult::new("test-server", "1.0.0"))
            }
        }

        // Test that we can connect multiple times (e.g., after disconnect)
        let (mut client, server1) = setup_client_server().await;

        // First connection is already established by setup_client_server
        client.ping().await.expect("First ping failed");

        // Drop the first server to simulate disconnect
        drop(server1);

        // Wait a bit for the connection to close
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Now create a new server and reconnect
        let (client_transport, server_transport) = TestTransport::create_pair();

        // Create a new test server
        let server = Server::default().with_connection(TestConnection::default);
        let _server2 = ServerHandle::new(server, server_transport)
            .await
            .expect("Failed to start second server");

        // Reconnect the client
        client
            .connect(client_transport)
            .await
            .expect("Reconnect failed");

        // Initialize and test the new connection
        client.init().await.expect("Re-initialize failed");
        client.ping().await.expect("Ping after reconnect failed");
    }
}
