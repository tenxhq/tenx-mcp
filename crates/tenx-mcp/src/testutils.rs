//! Test utilities for `tenx_mcp`.
//!
//! This module aggregates the helper types and functions that are useful when
//! writing unit and integration tests against this crate. Everything is kept
//! behind the `testutils` module so that the public API surface of the crate
//! remains clean while still making the helpers available to *external* test
//! crates via `use tenx_mcp::testutils::*`.
//!
//! The intent is **not** to provide a full-blown test framework but rather to
//! centralise the small bits of boiler-plate that were previously copied into
//! each individual test file (creation of in-emory duplex streams, sending
//! and receiving newline-delimited JSON-RPC messages, spinning up an in-process
//! server, …). Centralising this logic makes the tests shorter, avoids subtle
//! divergences, and gives downstream users example code they can re-use in
//! their own test suites.

use tokio::io::{self, AsyncRead, AsyncWrite};
use tokio::sync::broadcast;
use tracing_subscriber as _; // bring dependency for init_tracing helper

use crate::{
    error::Result,
    schema::{ClientNotification, ServerNotification},
    Client, ClientConn, ClientCtx, Server, ServerConn, ServerCtx, ServerHandle,
};

/// Conveniently create **two** independent in-memory duplex pipes that together
/// form a bidirectional channel suitable for wiring up a test client and
/// server.
///
/// The return value is laid out so that the first two elements can be given to
/// the server (`reader`, `writer`) and the remaining pair to the client. The
/// exact concrete stream types are hidden behind `impl Trait` so that callers
/// don't have to rely on the *exact* type (`tokio::io::DuplexStream`).
pub fn make_duplex_pair() -> (
    impl AsyncRead + Send + Sync + Unpin + 'static,
    impl AsyncWrite + Send + Sync + Unpin + 'static,
    impl AsyncRead + Send + Sync + Unpin + 'static,
    impl AsyncWrite + Send + Sync + Unpin + 'static,
) {
    // 8 KiB buffer on each side – more than enough for the very small test
    // messages we send around.
    let (server_reader, client_writer) = io::duplex(8 * 1024);
    let (client_reader, server_writer) = io::duplex(8 * 1024);
    (server_reader, server_writer, client_reader, client_writer)
}

/// Spin up an in-memory server using the supplied [`ServerConnection`]
/// factory, establish a connected [`Client`] (optionally with a custom
/// [`ClientConnection`]) and return both handles.
///
/// The helper takes care of wiring up the in-memory transport and saves the
/// caller from having to remember the exact incantations required to start the
/// server in the background.
pub async fn connected_client_and_server<F>(
    connection_factory: F,
) -> Result<(Client<()>, ServerHandle)>
where
    F: Fn() -> Box<dyn ServerConn> + Send + Sync + 'static,
{
    // Build server.
    let server = Server::default().with_connection_factory(connection_factory);

    // Two in-memory pipes to serve as the transport.
    let (server_reader, server_writer, client_reader, client_writer) = make_duplex_pair();

    // Start server.
    let server_handle = ServerHandle::from_stream(server, server_reader, server_writer).await?;

    // Build client instance.
    let mut client = Client::new("test-client", "1.0.0");

    // Connect the client to its side of the in-memory transport.
    client.connect_stream(client_reader, client_writer).await?;

    Ok((client, server_handle))
}

/// Helper function to create a connected client and server with a custom client connection
pub async fn connected_client_and_server_with_conn<F, C>(
    connection_factory: F,
    client_connection: C,
) -> Result<(Client<C>, ServerHandle)>
where
    F: Fn() -> Box<dyn ServerConn> + Send + Sync + 'static,
    C: ClientConn + 'static,
{
    // Build server.
    let server = Server::default().with_connection_factory(connection_factory);

    // Two in-memory pipes to serve as the transport.
    let (server_reader, server_writer, client_reader, client_writer) = make_duplex_pair();

    // Start server.
    let server_handle = ServerHandle::from_stream(server, server_reader, server_writer).await?;

    // Build client instance.
    let mut client = Client::new_with_connection("test-client", "1.0.0", client_connection);

    // Connect the client to its side of the in-memory transport.
    client.connect_stream(client_reader, client_writer).await?;

    Ok((client, server_handle))
}

/// Gracefully shut down a client–server pair previously created with
/// [`connected_client_and_server`]. The helper first drops the client so that
/// the underlying transport is closed and then waits (with a short timeout) for
/// the server task to notice the closed connection and terminate.
pub async fn shutdown_client_and_server<C>(client: Client<C>, server: ServerHandle)
where
    C: ClientConn + 'static,
{
    use tokio::time::{timeout, Duration};

    // Explicitly drop so that the transport is closed *before* we await the
    // server shutdown.
    drop(client);

    let _ = timeout(Duration::from_millis(10), server.stop()).await;
}

/// Create a ServerCtx for testing purposes.
/// This creates a ServerCtx with only notification capability (no request/response).
pub fn test_server_ctx(notification_tx: broadcast::Sender<ServerNotification>) -> ServerCtx {
    ServerCtx::new(notification_tx, None)
}

/// Create a ClientCtx for testing purposes.
/// This creates a ClientCtx with only notification capability (no request/response).
pub fn test_client_ctx(notification_tx: broadcast::Sender<ClientNotification>) -> ClientCtx {
    ClientCtx::new(notification_tx, None)
}

/// Test context for ServerConn implementations.
/// Provides a ServerCtx and channels for testing.
pub struct TestServerContext {
    pub ctx: ServerCtx,
    pub notification_tx: broadcast::Sender<ServerNotification>,
    pub notification_rx: broadcast::Receiver<ServerNotification>,
}

impl TestServerContext {
    /// Create a new test server context with notification channels
    pub fn new() -> Self {
        let (notification_tx, notification_rx) = broadcast::channel(100);
        let ctx = test_server_ctx(notification_tx.clone());
        Self {
            ctx,
            notification_tx,
            notification_rx,
        }
    }

    /// Get a reference to the ServerCtx
    pub fn ctx(&self) -> &ServerCtx {
        &self.ctx
    }

    /// Try to receive a notification, returning None if no notification is available
    pub async fn try_recv_notification(&mut self) -> Option<ServerNotification> {
        use tokio::time::{timeout, Duration};
        timeout(Duration::from_millis(10), self.notification_rx.recv())
            .await
            .ok()
            .and_then(|result| result.ok())
    }
}

impl Default for TestServerContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Test context for ClientConn implementations.
/// Provides a ClientCtx and channels for testing.
pub struct TestClientContext {
    pub ctx: ClientCtx,
    pub notification_tx: broadcast::Sender<ClientNotification>,
    pub notification_rx: broadcast::Receiver<ClientNotification>,
}

impl TestClientContext {
    /// Create a new test client context with notification channels
    pub fn new() -> Self {
        let (notification_tx, notification_rx) = broadcast::channel(100);
        let ctx = test_client_ctx(notification_tx.clone());
        Self {
            ctx,
            notification_tx,
            notification_rx,
        }
    }

    /// Get a reference to the ClientCtx
    pub fn ctx(&self) -> &ClientCtx {
        &self.ctx
    }

    /// Try to receive a notification, returning None if no notification is available
    pub async fn try_recv_notification(&mut self) -> Option<ClientNotification> {
        use tokio::time::{timeout, Duration};
        timeout(Duration::from_millis(10), self.notification_rx.recv())
            .await
            .ok()
            .and_then(|result| result.ok())
    }
}

impl Default for TestClientContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a standalone [`ServerCtx`] with its own broadcast channel.
pub fn new_server_ctx() -> ServerCtx {
    let (tx, _) = broadcast::channel(100);
    test_server_ctx(tx)
}

/// Create a standalone [`ClientCtx`] with its own broadcast channel.
pub fn new_client_ctx() -> ClientCtx {
    let (tx, _) = broadcast::channel(100);
    test_client_ctx(tx)
}

/// Initialize a tracing subscriber for tests.
/// It is safe to call this multiple times.
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt::try_init();
}
