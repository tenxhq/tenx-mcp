# MCP Examples

This directory contains example client and server implementations using the tenx-mcp library.

## Basic Server

A simple MCP server that listens for TCP connections and provides two example tools:
- `echo`: Echoes back the provided message
- `add`: Adds two numbers together

### Running the server:

```bash
# Run with default settings (127.0.0.1:3000)
cargo run --example basic_server

# Run on a specific host and port
cargo run --example basic_server -- 0.0.0.0 8080
```

## Basic Client

A complete MCP client that connects to a TCP server, initializes the connection, lists available tools, and optionally calls the echo tool if available.

### Running the client:

```bash
# Connect to default server (localhost:3000)
cargo run --example basic_client

# Connect to a specific host and port
cargo run --example basic_client -- localhost 8080
```

## Testing the Examples

1. Start the server in one terminal:
   ```bash
   cargo run --example basic_server
   ```

2. In another terminal, run the client:
   ```bash
   cargo run --example basic_client
   ```

The client will:
- Connect to the server
- Initialize the MCP protocol
- List available tools
- Call the echo tool (if available) 
- Listen for notifications
- Wait for Ctrl+C to exit

The server will:
- Listen for incoming connections
- Handle multiple clients concurrently
- Provide the echo and add tools
- Log all incoming requests