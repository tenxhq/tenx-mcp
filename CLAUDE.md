# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

tenx-mcp is a complete Rust implementation of the Model Context Protocol (MCP),
providing both client and server capabilities for building AI-integrated
applications. The project uses async/await patterns with Tokio and provides
ergonomic APIs through procedural macros.

## Model guidance

- Always run `cargo fmt` before submitting code to ensure consistent formatting.
- Prefer to write durable integration tests over running examples or creating disposable test scripts.

## Development Commands

### Building and Testing
```bash
# Build the project
cargo build

# Run all tests including workspace tests
cargo test --workspace

# Run tests with output (useful for debugging)
cargo test -- --nocapture

# Run a specific test
cargo test test_name

# Check code without building
cargo check

# Format code
cargo fmt

# Run linter
cargo clippy --examples --tests
```

### Running Examples
```bash
# Run server examples (default is TCP mode)
cargo run --example basic_server
cargo run --example basic_server stdio    # stdio transport
cargo run --example basic_server http     # HTTP transport

# Run client examples
cargo run --example basic_client
cargo run --example basic_client_stdio
```

## Architecture Overview

### Workspace Structure
- `crates/tenx-mcp/`: Main library implementing MCP protocol
- `crates/macros/`: Procedural macros for `#[mcp_server]` derive

### Core Components

1. **Protocol Implementation** (`crates/tenx-mcp/src/`)
   - `api.rs`: Core traits `ServerAPI` and `ClientAPI` defining MCP protocol methods
   - `schema.rs`: Complete MCP protocol schema definitions (JSON-RPC messages, types)
   - `client.rs` & `server.rs`: Client and server implementations
   - `transport.rs`: Transport abstractions supporting TCP, HTTP, and stdio

2. **Connection Management**
   - `connection.rs`: `ClientConn` and `ServerConn` for connection lifecycle
   - `context.rs`: Request-scoped context objects (`ServerCtx`, `ClientCtx`)
   - `request_handler.rs`: Request routing and handling infrastructure

3. **Transport Layers**
   - TCP/IP: Direct socket communication
   - HTTP: Uses Server-Sent Events (SSE) for server-to-client messages
   - Stdio: Communication via stdin/stdout for subprocess integration

### Key Design Patterns

1. **Macro-Driven Development**: The `#[mcp_server]` macro eliminates boilerplate:
   ```rust
   #[mcp_server]
   impl MyServer {
       #[tool]
       async fn my_tool(&self, ctx: &ServerCtx, params: MyParams) -> Result<CallToolResult> {
           // Implementation
       }
   }
   ```

2. **Trait-Based Architecture**: Core functionality defined through `ServerAPI`
   and `ClientAPI` traits, enabling flexible implementations and testing.

3. **Async-First**: All I/O operations are async using Tokio, supporting
   concurrent handling of multiple connections.

4. **Type Safety**: All protocol messages are strongly typed Rust structs with
   serde derives, ensuring compile-time correctness.

