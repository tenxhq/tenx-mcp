use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::{
    error::{Error, Result},
    schema,
};

/// Context provided to ClientConnection implementations for interacting with the client
#[derive(Debug, Clone)]
pub struct ClientConnectionContext {
    /// Sender for client notifications
    pub(crate) notification_tx: broadcast::Sender<schema::ServerNotification>,
}

impl ClientConnectionContext {
    /// Send a notification to the client
    pub fn send_notification(&self, notification: schema::ServerNotification) -> Result<()> {
        self.notification_tx.send(notification).map_err(|_| {
            crate::error::Error::InternalError("Failed to send notification".into())
        })?;
        Ok(())
    }
}

/// Connection trait that server implementers must implement
/// Each client connection will have its own instance of the implementation
#[async_trait]
pub trait ClientConnection: Send + Sync {
    /// Called when a new connection is established
    async fn on_connect(&mut self, _context: ClientConnectionContext) -> Result<()> {
        Ok(())
    }

    /// Called when the connection is being closed
    async fn on_disconnect(&mut self) -> Result<()> {
        Ok(())
    }

    async fn ping(&mut self) -> Result<()> {
        Ok(())
    }

    async fn create_message(
        &mut self,
        _method: &str,
        _params: schema::CreateMessageParams,
    ) -> Result<schema::CreateMessageResult> {
        Err(Error::InvalidRequest(
            "create_message not implemented".into(),
        ))
    }

    async fn list_roots(&mut self) -> Result<schema::ListRootsResult> {
        Err(Error::InvalidRequest("list_roots not implemented".into()))
    }

    /// Handle a notification sent from the server
    ///
    /// The default implementation ignores the notification. Implementations
    /// can override this method to react to server-initiated notifications.
    async fn notification(&mut self, _notification: schema::ClientNotification) -> Result<()> {
        Ok(())
    }
}
