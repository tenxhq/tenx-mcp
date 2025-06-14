//! Tests for graceful shutdown functionality

use async_trait::async_trait;
use std::time::Duration;
use tenx_mcp::{
    error::Result,
    server::MCPServer,
    transport::Transport,
};
use tokio::time::timeout;

// Test transport implementation
mod test_transport {
    use super::*;
    use tenx_mcp::codec::JsonRpcCodec;
    use tenx_mcp::transport::TransportStream;
    use tokio_util::codec::Framed;

    pub struct TestTransport {
        stream: Option<tokio::io::DuplexStream>,
    }

    impl TestTransport {
        pub fn new(stream: tokio::io::DuplexStream) -> Self {
            Self {
                stream: Some(stream),
            }
        }
    }

    #[async_trait]
    impl Transport for TestTransport {
        async fn connect(&mut self) -> Result<()> {
            Ok(())
        }

        fn framed(mut self: Box<Self>) -> Result<Box<dyn TransportStream>> {
            let stream = self.stream.take().unwrap();
            Ok(Box::new(Framed::new(stream, JsonRpcCodec::new())))
        }
    }
}

use test_transport::TestTransport;

#[tokio::test]
async fn test_server_shutdown_while_serving() {
    // Test that server can be shut down while serving
    let (_client_stream, server_stream) = tokio::io::duplex(8192);

    let mut server = MCPServer::new("serving-shutdown-server".to_string(), "1.0.0".to_string());

    // Get a shutdown receiver to test that the signal propagates
    let mut shutdown_rx = server
        .shutdown_receiver()
        .expect("Should get shutdown receiver");

    // Clone the server's shutdown sender for triggering shutdown
    let shutdown_sender = server.shutdown_sender();

    // Start server task
    let server_handle = tokio::spawn({
        async move {
            let transport: Box<dyn Transport> = Box::new(TestTransport::new(server_stream));
            server.serve(transport).await
        }
    });

    // Wait for shutdown signal to be received
    let shutdown_received = tokio::spawn(async move { shutdown_rx.recv().await.is_ok() });

    // Trigger shutdown
    let _ = shutdown_sender.send(());

    // Wait for either the server to complete or shutdown signal
    let shutdown_result = timeout(Duration::from_secs(2), shutdown_received)
        .await
        .expect("Should receive shutdown signal")
        .expect("Shutdown task should complete");

    assert!(shutdown_result, "Should receive shutdown signal");

    // Clean up server task
    server_handle.abort();
}
