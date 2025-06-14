use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{debug, info};

use crate::error::{MCPError, Result};
use crate::schema::*;
use crate::transport::{Transport, TransportStream};

/// Type for handling either a response or error from JSON-RPC
#[allow(dead_code)]
enum ResponseOrError {
    Response(JSONRPCResponse),
    Error(JSONRPCError),
}

/// MCP Client implementation
pub struct MCPClient {
    transport_tx: Option<mpsc::UnboundedSender<JSONRPCMessage>>,
    pending_requests: Arc<Mutex<HashMap<String, oneshot::Sender<ResponseOrError>>>>,
    notification_tx: mpsc::Sender<JSONRPCNotification>,
    notification_rx: Option<mpsc::Receiver<JSONRPCNotification>>,
    next_request_id: Arc<Mutex<u64>>,
}

impl MCPClient {
    /// Create a new MCP client
    pub fn new() -> Self {
        let (notification_tx, notification_rx) = mpsc::channel(100);

        Self {
            transport_tx: None,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            notification_tx,
            notification_rx: Some(notification_rx),
            next_request_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Connect using the provided transport
    pub async fn connect(&mut self, mut transport: Box<dyn Transport>) -> Result<()> {
        transport.connect().await?;
        let stream = transport.framed()?;

        let (transport_tx, transport_rx) = mpsc::unbounded_channel();
        self.transport_tx = Some(transport_tx);

        // Start the transport handler task
        self.start_transport_handler(stream, transport_rx).await;

        info!("MCP client connected");
        Ok(())
    }

    /// Initialize the connection with the server
    pub async fn initialize(
        &mut self,
        client_info: Implementation,
        capabilities: ClientCapabilities,
    ) -> Result<InitializeResult> {
        let params = InitializeParams {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            capabilities,
            client_info,
        };

        let value = self
            .request("initialize", Some(serde_json::to_value(params)?))
            .await;
        let result: InitializeResult = serde_json::from_value(value?)?;

        // Send the initialized notification to complete the handshake
        self.send_notification("notifications/initialized", None)
            .await?;

        Ok(result)
    }

    /// List available tools from the server
    pub async fn list_tools(&mut self) -> Result<ListToolsResult> {
        let value = self.request("tools/list", None).await?;
        let result: ListToolsResult = serde_json::from_value(value)?;
        Ok(result)
    }

    /// Call a tool on the server
    pub async fn call_tool(
        &mut self,
        name: String,
        arguments: Option<serde_json::Value>,
    ) -> Result<CallToolResult> {
        let params = CallToolParams {
            name,
            arguments: arguments.map(|args| {
                if let serde_json::Value::Object(map) = args {
                    map.into_iter().collect()
                } else {
                    std::collections::HashMap::new()
                }
            }),
        };

        let value = self
            .request("tools/call", Some(serde_json::to_value(params)?))
            .await?;
        let result: CallToolResult = serde_json::from_value(value)?;
        Ok(result)
    }

    /// Take the notification receiver channel
    pub fn take_notification_receiver(&mut self) -> Option<mpsc::Receiver<JSONRPCNotification>> {
        self.notification_rx.take()
    }

    /// Send a request and wait for response
    async fn request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let id = self.next_request_id().await;
        let (tx, rx) = oneshot::channel();

        // Store the response channel
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id.clone(), tx);
        }

        // Create the request params in the correct format
        let request_params = params.map(|v| RequestParams {
            meta: None,
            other: if let Some(obj) = v.as_object() {
                obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
            } else {
                HashMap::new()
            },
        });

        // Create and send the request
        let request = JSONRPCRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: RequestId::String(id.clone()),
            request: Request {
                method: method.to_string(),
                params: request_params,
            },
        };

        self.send_message(JSONRPCMessage::Request(request)).await?;

        // Wait for response
        match rx.await {
            Ok(response_or_error) => {
                match response_or_error {
                    ResponseOrError::Response(response) => {
                        // Extract result from the flattened Result structure
                        // For now, we'll return the whole result as JSON
                        Ok(serde_json::to_value(response.result)?)
                    }
                    ResponseOrError::Error(error) => Err(MCPError::Protocol(format!(
                        "JSON-RPC error {}: {}",
                        error.error.code, error.error.message
                    ))),
                }
            }
            Err(_) => Err(MCPError::Protocol("Response channel closed".to_string())),
        }
    }

    /// Send a message through the transport
    async fn send_message(&mut self, message: JSONRPCMessage) -> Result<()> {
        if let Some(tx) = &self.transport_tx {
            tx.send(message)
                .map_err(|_| MCPError::Transport("Transport channel closed".to_string()))?;
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

    /// Start the transport handler task that manages the transport stream
    async fn start_transport_handler(
        &mut self,
        mut stream: Box<dyn TransportStream>,
        mut transport_rx: mpsc::UnboundedReceiver<JSONRPCMessage>,
    ) {
        let pending_requests = self.pending_requests.clone();
        let notification_tx = self.notification_tx.clone();

        tokio::spawn(async move {
            debug!("Transport handler started");

            loop {
                tokio::select! {
                    // Handle outgoing messages
                    msg = transport_rx.recv() => {
                        match msg {
                            Some(json_msg) => {
                                if let Err(e) = stream.send(json_msg).await {
                                    break;
                                }
                            }
                            None => {
                                debug!("Transport channel closed");
                                break;
                            }
                        }
                    }

                    // Handle incoming messages
                    msg_result = stream.next() => {
                        match msg_result {
                            Some(Ok(message)) => {
                                match message {
                                    JSONRPCMessage::Response(response) => {
                                        // Handle response - match to pending request
                                        let id_str = match &response.id {
                                            RequestId::String(s) => s.clone(),
                                            RequestId::Number(n) => n.to_string(),
                                        };

                                        debug!("Received response for request ID: {}", id_str);

                                        // Find and remove the pending request
                                        let mut pending = pending_requests.lock().await;
                                        if let Some(tx) = pending.remove(&id_str) {
                                            if let Err(_) = tx.send(ResponseOrError::Response(response)) {
                                                debug!("Failed to send response - receiver dropped");
                                            }
                                        } else {
                                            debug!("No pending request found for ID: {}", id_str);
                                        }
                                    }
                                    JSONRPCMessage::Error(error) => {
                                        // Handle error response - match to pending request
                                        let id_str = match &error.id {
                                            RequestId::String(s) => s.clone(),
                                            RequestId::Number(n) => n.to_string(),
                                        };

                                        debug!("Received error response for request ID: {}", id_str);

                                        // Find and remove the pending request
                                        let mut pending = pending_requests.lock().await;
                                        if let Some(tx) = pending.remove(&id_str) {
                                            if let Err(_) = tx.send(ResponseOrError::Error(error)) {
                                                debug!("Failed to send error response - receiver dropped");
                                            }
                                        } else {
                                            debug!("No pending request found for error ID: {}", id_str);
                                        }
                                    }
                                    JSONRPCMessage::Notification(notification) => {
                                        // Handle notification
                                        debug!("Received notification: {}", notification.notification.method);
                                        if let Err(_) = notification_tx.send(notification).await {
                                            debug!("Failed to send notification - receiver dropped");
                                        }
                                    }
                                    _ => {
                                        debug!("Received unexpected message type: {:?}", message);
                                    }
                                }
                            }
                            Some(Err(e)) => {
                                debug!("Error reading message: {}", e);
                                // Connection error - we should close all pending requests
                                let mut pending = pending_requests.lock().await;
                                for (id, tx) in pending.drain() {
                                    debug!("Closing pending request {} due to connection error", id);
                                    let _ = tx.send(ResponseOrError::Error(JSONRPCError {
                                        jsonrpc: JSONRPC_VERSION.to_string(),
                                        id: RequestId::String(id),
                                        error: ErrorObject {
                                            code: -32603,
                                            message: format!("Connection error: {}", e),
                                            data: None,
                                        },
                                    }));
                                }
                                break;
                            }
                            None => {
                                // Stream ended
                                debug!("Transport stream ended");
                                break;
                            }
                        }
                    }
                }
            }

            debug!("Transport handler stopped");
        });
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

    #[test]
    fn test_client_creation() {
        let client = MCPClient::new();
        assert!(client.transport_tx.is_none());
    }

    #[tokio::test]
    async fn test_next_request_id() {
        let client = MCPClient::new();
        let id1 = client.next_request_id().await;
        let id2 = client.next_request_id().await;

        assert_eq!(id1, "req-1");
        assert_eq!(id2, "req-2");
    }
}
