use futures::SinkExt;
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
    transport: Option<Box<dyn TransportStream>>,
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
            transport: None,
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
        self.transport = Some(stream);

        // Start the message handler task
        self.start_message_handler().await;

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
            .await?;
        let result: InitializeResult = serde_json::from_value(value)?;
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
            arguments: arguments.and_then(|v| {
                v.as_object()
                    .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            }),
        };

        let value = self
            .request("tools/call", Some(serde_json::to_value(params)?))
            .await?;
        let result: CallToolResult = serde_json::from_value(value)?;
        Ok(result)
    }

    /// Get a receiver for notifications from the server
    pub fn take_notification_receiver(&mut self) -> Option<mpsc::Receiver<JSONRPCNotification>> {
        self.notification_rx.take()
    }

    /// Send a request to the server and wait for response
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
        if let Some(transport) = &mut self.transport {
            transport.send(message).await?;
            Ok(())
        } else {
            Err(MCPError::Transport("Not connected".to_string()))
        }
    }

    /// Generate the next request ID
    async fn next_request_id(&self) -> String {
        let mut id = self.next_request_id.lock().await;
        let current = *id;
        *id += 1;
        format!("req-{}", current)
    }

    /// Start the background task that handles incoming messages
    async fn start_message_handler(&self) {
        let _pending_requests = self.pending_requests.clone();
        let _notification_tx = self.notification_tx.clone();

        // This is a simplified version - in a real implementation,
        // you'd spawn a task that reads from the transport
        tokio::spawn(async move {
            debug!("Message handler started");
            // Message handling logic would go here
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
        assert!(client.transport.is_none());
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
