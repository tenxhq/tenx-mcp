use async_trait::async_trait;
use futures::{Sink, Stream};
use tokio::{
    io::{AsyncRead, AsyncWrite, BufReader},
    net::TcpStream,
};
use tokio_util::codec::Framed;
use tracing::info;

use crate::{
    codec::JsonRpcCodec,
    error::{Error, Result},
    schema::JSONRPCMessage,
};

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
    Stream<Item = Result<JSONRPCMessage>> + Sink<JSONRPCMessage, Error = Error> + Send + Unpin
{
}

/// A duplex wrapper around stdin/stdout for use with codec framing
pub struct StdioDuplex {
    reader: BufReader<tokio::io::Stdin>,
    writer: tokio::io::Stdout,
}

/// A generic duplex wrapper for combining separate AsyncRead and AsyncWrite streams
pub struct GenericDuplex<R, W> {
    reader: BufReader<R>,
    writer: W,
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

impl<R, W> GenericDuplex<R, W>
where
    R: AsyncRead,
{
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: BufReader::new(reader),
            writer,
        }
    }
}

impl<R, W> AsyncRead for GenericDuplex<R, W>
where
    R: AsyncRead + Unpin,
    W: Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

impl<R, W> AsyncWrite for GenericDuplex<R, W>
where
    R: Unpin,
    W: AsyncWrite + Unpin,
{
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
        info!("Stdio transport ready");
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

/// TCP client transport for outgoing network connections
pub struct TcpClientTransport {
    addr: String,
    stream: Option<TcpStream>,
}

impl TcpClientTransport {
    pub fn new(addr: impl Into<String>) -> Self {
        Self {
            addr: addr.into(),
            stream: None,
        }
    }
}

/// Wrapper to turn any AsyncRead + AsyncWrite stream into a Transport
pub struct StreamTransport<S> {
    stream: Option<S>,
}

impl<S> StreamTransport<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream: Some(stream),
        }
    }
}

#[async_trait]
impl Transport for TcpClientTransport {
    async fn connect(&mut self) -> Result<()> {
        info!("Connecting to TCP endpoint: {}", self.addr);
        let stream = TcpStream::connect(&self.addr).await?;
        self.stream = Some(stream);
        Ok(())
    }

    fn framed(self: Box<Self>) -> Result<Box<dyn TransportStream>> {
        let stream = self.stream.ok_or(Error::TransportDisconnected)?;

        let framed = Framed::new(stream, JsonRpcCodec::new());
        Ok(Box::new(framed))
    }
}

#[async_trait]
impl<S> Transport for StreamTransport<S>
where
    S: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static,
{
    async fn connect(&mut self) -> Result<()> {
        // Stream transports are already connected
        Ok(())
    }

    fn framed(self: Box<Self>) -> Result<Box<dyn TransportStream>> {
        let stream = self.stream.ok_or(Error::TransportDisconnected)?;
        let framed = Framed::new(stream, JsonRpcCodec::new());
        Ok(Box::new(framed))
    }
}

// Convenience implementation for TcpStream
#[async_trait]
impl Transport for TcpStream {
    async fn connect(&mut self) -> Result<()> {
        // TcpStream is already connected
        Ok(())
    }

    fn framed(self: Box<Self>) -> Result<Box<dyn TransportStream>> {
        let framed = Framed::new(*self, JsonRpcCodec::new());
        Ok(Box::new(framed))
    }
}

#[cfg(test)]
pub use test_transport::TestTransport;

#[cfg(test)]
pub(crate) mod test_transport {
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };

    use tokio::sync::mpsc;

    use super::*;

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
        type Error = Error;

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
            self.sender.send(item).map_err(|_| Error::ConnectionClosed)
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
    async fn test_tcp_client_transport_creation() {
        let transport = TcpClientTransport::new("localhost:8080");
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

    #[tokio::test]
    async fn test_generic_duplex() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        // Create two pairs of duplex streams
        let (reader1, writer1) = tokio::io::duplex(64);
        let (reader2, writer2) = tokio::io::duplex(64);

        // Create GenericDuplex instances that cross-connect
        let mut duplex1 = GenericDuplex::new(reader1, writer2);
        let mut duplex2 = GenericDuplex::new(reader2, writer1);

        // Test writing from duplex1 and reading from duplex2
        let data = b"Hello, world!";
        duplex1.write_all(data).await.unwrap();
        duplex1.flush().await.unwrap();

        let mut buf = vec![0u8; data.len()];
        duplex2.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, data);

        // Test the reverse direction
        let data2 = b"Response!";
        duplex2.write_all(data2).await.unwrap();
        duplex2.flush().await.unwrap();

        let mut buf2 = vec![0u8; data2.len()];
        duplex1.read_exact(&mut buf2).await.unwrap();
        assert_eq!(&buf2, data2);
    }

    #[tokio::test]
    async fn test_stream_transport_with_generic_duplex() {
        // Create duplex streams for testing
        let (reader, writer) = tokio::io::duplex(1024);
        let duplex = GenericDuplex::new(reader, writer);

        // Create StreamTransport
        let mut transport = StreamTransport::new(duplex);

        // Should connect successfully (no-op for StreamTransport)
        transport.connect().await.unwrap();

        // Should be able to create a framed stream
        let _framed = Box::new(transport).framed().unwrap();
    }
}
