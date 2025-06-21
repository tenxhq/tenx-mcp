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
