use async_trait::async_trait;
use futures::{Sink, Stream};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_util::codec::Framed;
use tracing::info;

use crate::codec::JsonRpcCodec;
use crate::error::{MCPError, Result};
use crate::schema::JSONRPCMessage;

/// Transport trait for different connection types
#[async_trait]
pub trait Transport: Send + Sync {
    /// Connect to the transport
    async fn connect(&mut self) -> Result<()>;

    /// Get a framed stream for reading/writing JSON-RPC messages
    fn framed(self: Box<Self>) -> Result<Box<dyn TransportStream>>;
}

/// Trait for a bidirectional stream of JSON-RPC messages
pub trait TransportStream:
    Stream<Item = Result<JSONRPCMessage>> + Sink<JSONRPCMessage, Error = MCPError> + Send + Unpin
{
}

/// Wrapper to implement TransportStream for any Framed type
impl<T> TransportStream for Framed<T, JsonRpcCodec> where T: AsyncRead + AsyncWrite + Send + Unpin {}

/// Standard I/O transport using stdin/stdout
pub struct StdioTransport;

impl StdioTransport {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn connect(&mut self) -> Result<()> {
        info!("Stdio transport ready");
        Ok(())
    }

    fn framed(self: Box<Self>) -> Result<Box<dyn TransportStream>> {
        // Create separate reader and writer
        let _stdin = tokio::io::stdin();
        let _stdout = tokio::io::stdout();

        // We need to create a combined stream that can read from stdin and write to stdout
        // For now, we'll use a simplified approach
        // In a production implementation, you might want to use tokio::io::duplex or similar

        // This is a placeholder - in practice, you'd need a proper duplex implementation
        Err(MCPError::Transport(
            "Stdio transport not fully implemented yet".to_string(),
        ))
    }
}

/// TCP transport for network connections
pub struct TcpTransport {
    addr: String,
    stream: Option<TcpStream>,
}

impl TcpTransport {
    pub fn new(addr: impl Into<String>) -> Self {
        Self {
            addr: addr.into(),
            stream: None,
        }
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn connect(&mut self) -> Result<()> {
        info!("Connecting to TCP endpoint: {}", self.addr);
        let stream = TcpStream::connect(&self.addr).await?;
        self.stream = Some(stream);
        Ok(())
    }

    fn framed(self: Box<Self>) -> Result<Box<dyn TransportStream>> {
        let stream = self
            .stream
            .ok_or_else(|| MCPError::Transport("TCP transport not connected".to_string()))?;

        let framed = Framed::new(stream, JsonRpcCodec::new());
        Ok(Box::new(framed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tcp_transport_creation() {
        let transport = TcpTransport::new("localhost:8080");
        assert_eq!(transport.addr, "localhost:8080");
    }

    #[test]
    fn test_stdio_transport_creation() {
        let _transport = StdioTransport::new();
        // Just ensure it can be created
    }
}
