use async_trait::async_trait;
use tenx_mcp::error::Result;
use tenx_mcp::{schema::*, server_connection::ServerConnection};

#[tokio::test]
async fn test_server_to_client_notifications() {
    // Initialize tracing for debugging
    let _ = tracing_subscriber::fmt::try_init();

    use tenx_mcp::testutils::connected_client_and_server;
    use tokio::sync::oneshot;

    // Channel to signal when notification is received
    let (tx_notif, rx_notif) = oneshot::channel::<()>();

    // Custom client connection that records notifications
    struct NotificationRecorder {
        tx: Option<oneshot::Sender<()>>,
    }

    #[async_trait]
    impl tenx_mcp::client_connection::ClientConnection for NotificationRecorder {
        async fn notification(
            &mut self,
            _context: tenx_mcp::client_connection::ClientConnectionContext,
            notification: tenx_mcp::schema::ClientNotification,
        ) -> Result<()> {
            tracing::info!("Client received notification: {:?}", notification);
            if matches!(
                notification,
                tenx_mcp::schema::ClientNotification::RootsListChanged
            ) {
                if let Some(tx) = self.tx.take() {
                    let _ = tx.send(());
                }
            }
            Ok(())
        }
    }

    // Create tenx-mcp server that sends notifications
    struct NotifyingServer {
        sent_notification: std::sync::Arc<std::sync::Mutex<bool>>,
    }

    #[async_trait]
    impl ServerConnection for NotifyingServer {
        async fn on_connect(
            &mut self,
            context: tenx_mcp::server_connection::ServerConnectionContext,
        ) -> Result<()> {
            // Send a notification after connection
            let sent_notification = self.sent_notification.clone();
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                // Send roots list changed notification
                match context
                    .send_notification(tenx_mcp::schema::ClientNotification::RootsListChanged)
                {
                    Ok(_) => {
                        tracing::info!("Server sent roots_list_changed notification");
                        *sent_notification.lock().unwrap() = true;
                    }
                    Err(e) => {
                        tracing::error!("Failed to send notification: {:?}", e);
                    }
                }
            });
            Ok(())
        }

        async fn initialize(
            &mut self,
            _context: tenx_mcp::server_connection::ServerConnectionContext,
            _protocol_version: String,
            _capabilities: ClientCapabilities,
            _client_info: Implementation,
        ) -> Result<InitializeResult> {
            Ok(InitializeResult::new("notifying-server", "0.1.0"))
        }
    }

    // Track if notification was sent
    let sent_notification = std::sync::Arc::new(std::sync::Mutex::new(false));

    // Create connected client and server
    let (client, server_handle) = connected_client_and_server(
        {
            let sent_notification = sent_notification.clone();
            move || {
                Box::new(NotifyingServer {
                    sent_notification: sent_notification.clone(),
                })
            }
        },
        Some(Box::new(NotificationRecorder { tx: Some(tx_notif) })),
    )
    .await
    .expect("Failed to connect client and server");

    // Wait for the notification
    let result = tokio::time::timeout(tokio::time::Duration::from_secs(3), rx_notif).await;

    // Verify notification was sent by server
    assert!(
        *sent_notification.lock().unwrap(),
        "Server did not send notification"
    );

    // Verify notification was received by client
    assert!(result.is_ok(), "Timeout waiting for notification");
    assert!(result.unwrap().is_ok(), "Failed to receive notification");

    // Cleanup
    tenx_mcp::testutils::shutdown_client_and_server(client, server_handle).await;
}
