use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::timeout;
use tracing::{debug, error, info, warn};

use crate::{
    error::{MCPError, Result},
    retry::RetryConfig,
    schema::*,
    transport::{Transport, TransportStream},
};

/// Type for handling either a response or error from JSON-RPC
enum ResponseOrError {
    Response(JSONRPCResponse),
    Error(JSONRPCError),
}

/// Configuration for the MCP client
#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// Retry configuration for requests
    pub retry: RetryConfig,
    /// Default timeout for requests
    pub request_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            retry: RetryConfig::default(),
            request_timeout: Duration::from_secs(30),
        }
    }
}

type TransportSink = Arc<Mutex<SplitSink<Box<dyn TransportStream>, JSONRPCMessage>>>;

/// MCP Client implementation
pub struct MCPClient {
    transport_tx: Option<TransportSink>,
    pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<ResponseOrError>>>>,
    notification_tx: mpsc::Sender<JSONRPCNotification>,
    notification_rx: Option<mpsc::Receiver<JSONRPCNotification>>,
    next_request_id: Arc<Mutex<u64>>,
    config: ClientConfig,
}

impl MCPClient {
    /// Create a new MCP client with default configuration
    pub fn new() -> Self {
        Self::with_config(ClientConfig::default())
    }

    /// Create a new MCP client with custom configuration
    pub fn with_config(config: ClientConfig) -> Self {
        let (notification_tx, notification_rx) = mpsc::channel(100);

        Self {
            transport_tx: None,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            notification_tx,
            notification_rx: Some(notification_rx),
            next_request_id: Arc::new(Mutex::new(1)),
            config,
        }
    }

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
    pub async fn initialize(
        &mut self,
        client_info: Implementation,
        capabilities: ClientCapabilities,
    ) -> Result<InitializeResult> {
        let request = ClientRequest::Initialize {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            capabilities,
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
        self.request(ClientRequest::ListTools).await
    }

    /// Call a tool on the server
    pub async fn call_tool(
        &mut self,
        name: String,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        let arguments = arguments.map(|args| {
            if let serde_json::Value::Object(map) = args {
                map.into_iter().collect()
            } else {
                std::collections::HashMap::new()
            }
        });

        let request = ClientRequest::CallTool { name, arguments };
        self.request_with_retry(request).await
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

    /// Send a request with retry logic
    async fn request_with_retry<T>(&mut self, request: ClientRequest) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        // For now, we'll just do a single request without retry
        // TODO: Implement proper retry logic that doesn't require mutable self in closure
        self.request(request).await
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
        match timeout(self.config.request_timeout, rx).await {
            Ok(Ok(response_or_error)) => {
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
                            |e| MCPError::Protocol(format!("Failed to deserialize response: {e}")),
                        )
                    }
                    ResponseOrError::Error(error) => {
                        // Map JSON-RPC errors to appropriate MCPError variants
                        match error.error.code {
                            METHOD_NOT_FOUND => Err(MCPError::MethodNotFound(error.error.message)),
                            INVALID_PARAMS => Err(MCPError::invalid_params(
                                request.method(),
                                error.error.message,
                            )),
                            _ => Err(MCPError::Protocol(format!(
                                "JSON-RPC error {}: {}",
                                error.error.code, error.error.message
                            ))),
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                error!("Response channel closed for request {}: {}", id, e);
                // Remove the pending request
                self.pending_requests.lock().await.remove(&id);
                Err(MCPError::Protocol("Response channel closed".to_string()))
            }
            Err(_) => {
                // Timeout occurred
                error!(
                    "Request {} timed out after {:?}",
                    id, self.config.request_timeout
                );
                // Remove the pending request
                self.pending_requests.lock().await.remove(&id);
                Err(MCPError::timeout(self.config.request_timeout, id))
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
            Err(MCPError::Transport("Not connected".to_string()))
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

        // Spawn a task to handle incoming messages
        tokio::spawn(async move {
            debug!("Message handler started");

            while let Some(result) = rx.next().await {
                match result {
                    Ok(message) => {
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
                                // Forward notifications to the notification channel
                                if let Err(e) = notification_tx.send(notification).await {
                                    error!("Failed to send notification: {}", e);
                                    // If the receiver is dropped, we should stop
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
                                // Handle ping requests from server
                                if request.request.method == "ping" {
                                    info!("Received ping request from server with id: {:?}, sending response", request.id);

                                    // Create a response with empty result
                                    let response = JSONRPCResponse {
                                        jsonrpc: crate::schema::JSONRPC_VERSION.to_string(),
                                        id: request.id,
                                        result: EmptyResult {
                                            meta: None,
                                            other: HashMap::new(),
                                        },
                                    };

                                    // Send the response back through the transport
                                    let mut sink = tx.lock().await;
                                    match sink.send(JSONRPCMessage::Response(response)).await {
                                        Ok(_) => info!("Successfully sent ping response to server"),
                                        Err(e) => error!("Failed to send ping response: {}", e),
                                    }
                                } else {
                                    // Other requests are unexpected
                                    warn!(
                                        "Received unexpected request from server: {}",
                                        request.request.method
                                    );
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
                    Err(e) => {
                        error!("Error receiving message: {}", e);
                        // On error, we should probably break the loop
                        break;
                    }
                }
            }

            info!("Message handler stopped");
        });

        Ok(())
    }
}

impl Default for MCPClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{MCPServer, MCPServerHandle};
    use crate::transport::TestTransport;

    async fn setup_client_server() -> (MCPClient, MCPServerHandle) {
        let (client_transport, server_transport) = TestTransport::create_pair();

        // Create a minimal test connection
        struct TestConnection;

        #[async_trait::async_trait]
        impl crate::connection::Connection for TestConnection {
            async fn initialize(
                &mut self,
                _protocol_version: String,
                _capabilities: ClientCapabilities,
                _client_info: Implementation,
            ) -> Result<InitializeResult> {
                Ok(InitializeResult::new("test-server", "1.0.0"))
            }
        }

        let server = MCPServer::default().with_connection_factory(|| Box::new(TestConnection));
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

    #[test]
    fn test_client_creation() {
        let client = MCPClient::new();
        assert!(client.transport_tx.is_none());
    }

    #[tokio::test]
    async fn test_next_request_id() {
        let client = MCPClient::new();
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
}
