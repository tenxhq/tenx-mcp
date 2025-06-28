use async_trait::async_trait;
use tenx_mcp::{ClientConn, ClientCtx, Result, ServerConn, ServerCtx, schema};

#[tokio::test]
async fn test_server_to_client_notifications() {
    let _ = tracing_subscriber::fmt::try_init();

    use tenx_mcp::testutils::connected_client_and_server_with_conn;
    use tokio::sync::oneshot;

    let (tx_notif, rx_notif) = oneshot::channel::<()>();

    #[derive(Clone)]
    struct NotificationRecorder {
        tx: std::sync::Arc<std::sync::Mutex<Option<oneshot::Sender<()>>>>,
    }

    #[async_trait]
    impl ClientConn for NotificationRecorder {
        async fn notification(
            &self,
            _context: &ClientCtx,
            notification: tenx_mcp::schema::ServerNotification,
        ) -> Result<()> {
            tracing::info!("Client received notification: {:?}", notification);
            if matches!(
                notification,
                tenx_mcp::schema::ServerNotification::ToolListChanged
            ) {
                if let Some(tx) = self.tx.lock().unwrap().take() {
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
    impl ServerConn for NotifyingServer {
        async fn on_connect(&self, context: &ServerCtx, _remote_addr: &str) -> Result<()> {
            // Send a notification after connection
            let sent_notification = self.sent_notification.clone();
            let context = context.clone();
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                // Send roots list changed notification
                match context.notify(schema::ServerNotification::ToolListChanged) {
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
            &self,
            _context: &ServerCtx,
            _protocol_version: String,
            _capabilities: schema::ClientCapabilities,
            _client_info: schema::Implementation,
        ) -> Result<schema::InitializeResult> {
            Ok(schema::InitializeResult::new("notifying-server").with_version("0.1.0"))
        }
    }

    // Track if notification was sent
    let sent_notification = std::sync::Arc::new(std::sync::Mutex::new(false));

    // Create connected client and server
    let (mut client, server_handle) = connected_client_and_server_with_conn(
        {
            let sent_notification = sent_notification.clone();
            move || {
                Box::new(NotifyingServer {
                    sent_notification: sent_notification.clone(),
                })
            }
        },
        NotificationRecorder {
            tx: std::sync::Arc::new(std::sync::Mutex::new(Some(tx_notif))),
        },
    )
    .await
    .expect("Failed to connect client and server");

    // Initialize the connection to trigger on_connect
    client.init().await.expect("Failed to initialize");

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
