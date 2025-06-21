use tokio::sync::broadcast;

use crate::{
    api::ClientAPI,
    error::{Error, Result},
    request_handler::{RequestHandler, TransportSink},
    schema,
};

use async_trait::async_trait;

/// Context provided to ClientConnection implementations for interacting with the client
#[derive(Debug, Clone)]
pub struct ClientCtx {
    /// Sender for client notifications
    pub(crate) notification_tx: broadcast::Sender<schema::ClientNotification>,
}

impl ClientCtx {
    /// Create a new ClientConnectionContext with the given notification sender
    pub fn new(notification_tx: broadcast::Sender<schema::ClientNotification>) -> Self {
        Self { notification_tx }
    }

    /// Send a notification to the client
    pub fn send_notification(&self, notification: schema::ClientNotification) -> Result<()> {
        self.notification_tx
            .send(notification)
            .map_err(|_| Error::InternalError("Failed to send notification".into()))?;
        Ok(())
    }
}

/// Context provided to ServerConn implementations for interacting with clients
#[derive(Clone)]
pub struct ServerCtx {
    /// Sender for server notifications
    pub(crate) notification_tx: broadcast::Sender<schema::ServerNotification>,
    /// Request handler for making requests to clients
    request_handler: RequestHandler,
}

impl ServerCtx {
    /// Create a new ServerCtx with notification channel and transport
    pub(crate) fn new(
        notification_tx: broadcast::Sender<schema::ServerNotification>,
        transport_tx: Option<TransportSink>,
    ) -> Self {
        Self {
            notification_tx,
            request_handler: RequestHandler::new(transport_tx, "srv-req".to_string()),
        }
    }

    /// Send a notification to the client
    pub fn notify(&self, notification: schema::ServerNotification) -> Result<()> {
        self.notification_tx
            .send(notification)
            .map_err(|_| Error::InternalError("Failed to send notification".into()))?;
        Ok(())
    }

    /// Send a request to the client and wait for response
    async fn request<T>(&mut self, request: schema::ServerRequest) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.request_handler.request(request).await
    }

    /// Handle a response from the client
    pub(crate) async fn handle_client_response(&self, response: schema::JSONRPCResponse) {
        // Clone the handler to avoid holding locks across await points
        let handler = self.request_handler.clone();
        handler.handle_response(response).await
    }

    /// Handle an error response from the client
    pub(crate) async fn handle_client_error(&self, error: schema::JSONRPCError) {
        // Clone the handler to avoid holding locks across await points
        let handler = self.request_handler.clone();
        handler.handle_error(error).await
    }
}

/// Implementation of ClientAPI trait for ServerCtx
#[async_trait]
impl ClientAPI for ServerCtx {
    async fn ping(&mut self) -> Result<()> {
        let _: schema::EmptyResult = self.request(schema::ServerRequest::Ping).await?;
        Ok(())
    }

    async fn create_message(
        &mut self,
        params: schema::CreateMessageParams,
    ) -> Result<schema::CreateMessageResult> {
        self.request(schema::ServerRequest::CreateMessage(Box::new(params)))
            .await
    }

    async fn list_roots(&mut self) -> Result<schema::ListRootsResult> {
        self.request(schema::ServerRequest::ListRoots).await
    }
}

