use async_trait::async_trait;
use futures::{Sink, Stream};
use tokio::io::{AsyncRead, AsyncWrite, BufReader};
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

/// A duplex wrapper around stdin/stdout for use with codec framing
pub struct StdioDuplex {
    reader: BufReader<tokio::io::Stdin>,
    writer: tokio::io::Stdout,
}

impl StdioDuplex {
    pub fn new(stdin: tokio::io::Stdin, stdout: tokio::io::Stdout) -> Self {
        Self {
            reader: BufReader::new(stdin),
            writer: stdout,
        }
    }
}

impl AsyncRead for StdioDuplex {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

impl AsyncWrite for StdioDuplex {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::result::Result<usize, std::io::Error>> {
        std::pin::Pin::new(&mut self.writer).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        std::pin::Pin::new(&mut self.writer).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        std::pin::Pin::new(&mut self.writer).poll_shutdown(cx)
    }
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
        // info!("Stdio transport ready");
        Ok(())
    }

    fn framed(self: Box<Self>) -> Result<Box<dyn TransportStream>> {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let duplex = StdioDuplex::new(stdin, stdout);
        let framed = Framed::new(duplex, JsonRpcCodec::new());
        Ok(Box::new(framed))
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
pub use test_transport::TestTransport;

#[cfg(test)]
mod test_transport {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::sync::mpsc;

    /// Test transport for unit testing
    pub struct TestTransport {
        sender: mpsc::UnboundedSender<JSONRPCMessage>,
        receiver: mpsc::UnboundedReceiver<JSONRPCMessage>,
    }

    impl TestTransport {
        /// Create a pair of connected test transports
        pub fn create_pair() -> (Box<dyn Transport>, Box<dyn Transport>) {
            let (tx1, rx1) = mpsc::unbounded_channel();
            let (tx2, rx2) = mpsc::unbounded_channel();

            let transport1 = Box::new(TestTransport {
                sender: tx2,
                receiver: rx1,
            });

            let transport2 = Box::new(TestTransport {
                sender: tx1,
                receiver: rx2,
            });

            (transport1, transport2)
        }
    }

    #[async_trait]
    impl Transport for TestTransport {
        async fn connect(&mut self) -> Result<()> {
            Ok(())
        }

        fn framed(self: Box<Self>) -> Result<Box<dyn TransportStream>> {
            Ok(Box::new(TestTransportStream {
                sender: self.sender,
                receiver: self.receiver,
            }))
        }
    }

    struct TestTransportStream {
        sender: mpsc::UnboundedSender<JSONRPCMessage>,
        receiver: mpsc::UnboundedReceiver<JSONRPCMessage>,
    }

    impl Stream for TestTransportStream {
        type Item = Result<JSONRPCMessage>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            match self.receiver.poll_recv(cx) {
                Poll::Ready(Some(msg)) => Poll::Ready(Some(Ok(msg))),
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }
    }

    impl Sink<JSONRPCMessage> for TestTransportStream {
        type Error = MCPError;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(
            self: Pin<&mut Self>,
            item: JSONRPCMessage,
        ) -> std::result::Result<(), Self::Error> {
            self.sender
                .send(item)
                .map_err(|_| MCPError::Transport("Failed to send message".to_string()))
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    impl TransportStream for TestTransportStream {}
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

    #[tokio::test]
    async fn test_test_transport_pair() {
        let (mut t1, mut t2) = TestTransport::create_pair();

        // Both should connect successfully
        t1.connect().await.unwrap();
        t2.connect().await.unwrap();
    }
}
