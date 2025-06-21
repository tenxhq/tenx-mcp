# HTTP Transport for tenx-mcp

This document describes the HTTP transport implementation for the Model Context Protocol (MCP) in tenx-mcp.

## Overview

The HTTP transport provides a complete implementation of the MCP Streamable HTTP transport specification (protocol version 2025-06-18). It supports:

- HTTP POST for client-to-server messages
- Server-Sent Events (SSE) for streaming responses
- Session management with persistent connections
- Multiple concurrent clients
- Full compliance with the MCP protocol specification

## Features

- **Bidirectional Communication**: Supports both request/response and server-initiated messages
- **SSE Streaming**: Automatic SSE support for long-running operations
- **Session Management**: Built-in session tracking with unique session IDs
- **Error Handling**: Robust error handling with proper HTTP status codes
- **Easy Integration**: Simple APIs for both client and server

## Usage

### HTTP Server

```rust
use tenx_mcp::{Server, ServerConn, ServerCtx, Result, start_http_server, ServerHandle};

// Create your server connection handler
struct MyServerConn;

#[async_trait]
impl ServerConn for MyServerConn {
    // Implement required methods...
}

#[tokio::main]
async fn main() -> Result<()> {
    // Start HTTP server on port 8080
    let transport = start_http_server("127.0.0.1:8080").await?;
    
    // Create MCP server with connection handler
    let server = Server::default()
        .with_connection(|| MyServerConn);
    
    // Serve using HTTP transport
    let handle = ServerHandle::from_transport(server, Box::new(transport)).await?;
    
    // Server is now running...
    handle.stop().await?;
    
    Ok(())
}
```

### HTTP Client

```rust
use tenx_mcp::{Client, Result, ServerAPI};

#[tokio::main]
async fn main() -> Result<()> {
    // Create client
    let mut client = Client::new("my-client", "1.0.0");
    
    // Connect via HTTP
    let init_result = client.connect_http("http://127.0.0.1:8080").await?;
    
    println!("Connected to: {}", init_result.server_info.name);
    
    // Use the client
    client.ping().await?;
    
    Ok(())
}
```

## Transport Details

### Request Flow

1. Client sends HTTP POST with JSON-RPC message
2. Server processes the request
3. Server responds with either:
   - Direct JSON response (for simple requests)
   - SSE stream (for requests that may generate multiple messages)

### Session Management

- Sessions are automatically created during initialization
- Session IDs are passed via the `Mcp-Session-Id` header
- Sessions persist across multiple requests
- Clients can explicitly terminate sessions with HTTP DELETE

### SSE Support

The transport automatically uses SSE when:
- The client accepts `text/event-stream`
- The server needs to send multiple messages
- Server-initiated notifications are required

### Error Handling

- HTTP 400: Bad request (malformed JSON, missing session ID)
- HTTP 404: Session not found
- HTTP 405: Method not allowed
- HTTP 202: Accepted (for notifications/responses)

## Examples

See the `examples/` directory for complete working examples:
- `http_server.rs`: Basic HTTP server with echo tool
- `http_client.rs`: Client that connects and uses the echo tool

To run the examples:

```bash
# Terminal 1: Start the server
cargo run --example http_server

# Terminal 2: Run the client
cargo run --example http_client
```

## Security Considerations

When implementing HTTP transport:

1. **Bind to localhost only** for local servers
2. **Validate Origin headers** to prevent DNS rebinding
3. **Implement authentication** for production use
4. **Use HTTPS** in production environments

## Compatibility

This implementation is compatible with:
- MCP Protocol Version: 2025-06-18
- Other MCP implementations following the Streamable HTTP transport spec
- The rmcp Rust SDK (when it adds HTTP support)