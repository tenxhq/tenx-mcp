use async_trait::async_trait;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use tenx_mcp::{schema::*, testutils::*, ClientConn, ClientCtx, Result, ServerConn, ServerCtx};

#[derive(Debug, Clone)]
struct LifecycleTestClient {
    connect_count: Arc<AtomicU32>,
    disconnect_count: Arc<AtomicU32>,
}

impl Default for LifecycleTestClient {
    fn default() -> Self {
        Self {
            connect_count: Arc::new(AtomicU32::new(0)),
            disconnect_count: Arc::new(AtomicU32::new(0)),
        }
    }
}

#[async_trait]
impl ClientConn for LifecycleTestClient {
    async fn on_connect(&self, _ctx: &ClientCtx) -> Result<()> {
        self.connect_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn on_disconnect(&self, _ctx: &ClientCtx) -> Result<()> {
        self.disconnect_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

// Simple server for testing
#[derive(Debug, Clone, Default)]
struct TestServer;

#[async_trait]
impl ServerConn for TestServer {
    async fn initialize(
        &self,
        _context: &ServerCtx,
        _protocol_version: String,
        _capabilities: ClientCapabilities,
        _client_info: Implementation,
    ) -> Result<InitializeResult> {
        Ok(InitializeResult::new("test_server").with_version("1.0.0"))
    }
}

#[tokio::test]
async fn test_client_lifecycle() {
    let client_conn = LifecycleTestClient::default();

    // Before connection, counts should be zero
    assert_eq!(
        client_conn.connect_count.load(Ordering::SeqCst),
        0,
        "Expected 0 client on_connect calls before connection"
    );
    assert_eq!(
        client_conn.disconnect_count.load(Ordering::SeqCst),
        0,
        "Expected 0 client on_disconnect calls before connection"
    );

    let (client, handle) = connected_client_and_server_with_conn(
        || Box::new(TestServer::default()) as Box<dyn ServerConn>,
        client_conn.clone(),
    )
    .await
    .unwrap();

    // Wait for connection to be established
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // After connection, client on_connect should have been called
    assert_eq!(
        client_conn.connect_count.load(Ordering::SeqCst),
        1,
        "Expected 1 client on_connect call after connection"
    );
    assert_eq!(
        client_conn.disconnect_count.load(Ordering::SeqCst),
        0,
        "Expected 0 client on_disconnect calls while connected"
    );

    // Disconnect
    shutdown_client_and_server(client, handle).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // After disconnect, client on_disconnect should have been called
    assert_eq!(
        client_conn.connect_count.load(Ordering::SeqCst),
        1,
        "Client connect count should remain 1 after disconnect"
    );
    assert_eq!(
        client_conn.disconnect_count.load(Ordering::SeqCst),
        1,
        "Expected 1 client on_disconnect call after shutdown"
    );
}

#[tokio::test]
async fn test_client_multiple_connections() {
    let client_conn1 = LifecycleTestClient::default();
    let client_conn2 = LifecycleTestClient::default();

    // Before any connections
    assert_eq!(client_conn1.connect_count.load(Ordering::SeqCst), 0);
    assert_eq!(client_conn1.disconnect_count.load(Ordering::SeqCst), 0);
    assert_eq!(client_conn2.connect_count.load(Ordering::SeqCst), 0);
    assert_eq!(client_conn2.disconnect_count.load(Ordering::SeqCst), 0);

    // Create first connection
    let (client1, handle1) = connected_client_and_server_with_conn(
        || Box::new(TestServer::default()) as Box<dyn ServerConn>,
        client_conn1.clone(),
    )
    .await
    .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // After first connection
    assert_eq!(
        client_conn1.connect_count.load(Ordering::SeqCst),
        1,
        "First client should have 1 on_connect call"
    );
    assert_eq!(
        client_conn1.disconnect_count.load(Ordering::SeqCst),
        0,
        "First client should have 0 on_disconnect calls"
    );
    assert_eq!(
        client_conn2.connect_count.load(Ordering::SeqCst),
        0,
        "Second client should still have 0 on_connect calls"
    );

    // Create second connection
    let (client2, handle2) = connected_client_and_server_with_conn(
        || Box::new(TestServer::default()) as Box<dyn ServerConn>,
        client_conn2.clone(),
    )
    .await
    .unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // After second connection
    assert_eq!(
        client_conn1.connect_count.load(Ordering::SeqCst),
        1,
        "First client should still have 1 on_connect call"
    );
    assert_eq!(
        client_conn2.connect_count.load(Ordering::SeqCst),
        1,
        "Second client should now have 1 on_connect call"
    );

    // Disconnect first client
    shutdown_client_and_server(client1, handle1).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // After first disconnect
    assert_eq!(
        client_conn1.disconnect_count.load(Ordering::SeqCst),
        1,
        "First client should have 1 on_disconnect call"
    );
    assert_eq!(
        client_conn2.disconnect_count.load(Ordering::SeqCst),
        0,
        "Second client should still have 0 on_disconnect calls"
    );

    // Disconnect second client
    shutdown_client_and_server(client2, handle2).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // After second disconnect
    assert_eq!(
        client_conn2.disconnect_count.load(Ordering::SeqCst),
        1,
        "Second client should now have 1 on_disconnect call"
    );
}
