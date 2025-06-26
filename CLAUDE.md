## Project Overview

tenx-mcp is a complete Rust implementation of the Model Context Protocol (MCP),
providing both client and server capabilities for building AI-integrated
applications. The project uses async/await patterns with Tokio and provides
ergonomic APIs through procedural macros.

## Model guidance

- Always run `cargo fmt` before submitting code to ensure consistent formatting.
- Always run `cargo clippy --tests --examples` to catch common mistakes and improve code quality.
    - You may run `cargo clippy --fix --tests --examples --allow-dirty` to automatically fix some issues.
- Prefer to write durable integration tests over running examples or creating disposable test scripts.
    - Integration tests go in ./crates/tenx-mcp/tests.

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

