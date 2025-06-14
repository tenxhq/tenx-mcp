use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::{
    error::{MCPError, Result},
    schema::*,
    transport::{Transport, TransportStream},
};

/// Type for handling either a response or error from JSON-RPC
enum ResponseOrError {
    Response(JSONRPCResponse),
    Error(JSONRPCError),
}

/// MCP Client implementation
pub struct MCPClient {
    transport_tx: Option<SplitSink<Box<dyn TransportStream>, JSONRPCMessage>>,
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

        let value = self.request(request).await?;
        let result: InitializeResult = serde_json::from_value(value)?;

        // Send the initialized notification to complete the handshake
        self.send_notification("notifications/initialized", None)
            .await?;

        Ok(result)
    }

    /// List available tools from the server
    pub async fn list_tools(&mut self) -> Result<ListToolsResult> {
        let value = self.request(ClientRequest::ListTools).await?;
        let result: ListToolsResult = serde_json::from_value(value)?;
        Ok(result)
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
        let value = self.request(request).await?;
        let result: CallToolResult = serde_json::from_value(value)?;
        Ok(result)
    }

    /// Take the notification receiver channel
    pub fn take_notification_receiver(&mut self) -> Option<mpsc::Receiver<JSONRPCNotification>> {
        self.notification_rx.take()
    }

    /// Send a request and wait for response
    async fn request(&mut self, request: ClientRequest) -> Result<serde_json::Value> {
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
            Err(e) => {
                error!("Response channel closed: {}", e);
                Err(MCPError::Protocol("Response channel closed".to_string()))
            }
        }
    }

    /// Send a message through the transport
    async fn send_message(&mut self, message: JSONRPCMessage) -> Result<()> {
        if let Some(transport_tx) = &mut self.transport_tx {
            transport_tx.send(message).await?;
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

        // Store the sender half for sending messages
        self.transport_tx = Some(tx);

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
                            JSONRPCMessage::Request(_request) => {
                                // Clients typically don't receive requests from servers in MCP
                                warn!("Received unexpected request from server");
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
