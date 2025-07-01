![Discord](https://img.shields.io/discord/1381424110831145070?style=flat-square&logo=rust&link=https%3A%2F%2Fdiscord.gg%2FfHmRmuBDxF)
[![Crates.io](https://img.shields.io/crates/v/tenx-mcp)](https://crates.io/crates/tenx-mcp)
[![docs.rs](https://img.shields.io/docsrs/tenx-mcp)](https://docs.rs/tenx-mcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

# tenx-mcp

A Rust implementation of the Model Context Protocol for building AI-integrated applications.

--- 

## Community

Want to contribute? Have ideas or feature requests? Come tell us about it on
[Discord](https://discord.gg/fHmRmuBDxF). 

---

## Overview

A Rust implementation of the Model Context Protocol (MCP) - a JSON-RPC 2.0 based
protocol for AI models to interact with external tools and services. Supports
both client and server roles with async/await APIs.

---

## Features

- **Full MCP Protocol Support**: Implements the latest MCP specification (2025-06-18)
- **Client & Server**: Build both MCP clients and servers with ergonomic APIs
- **Multiple Transports**: TCP/IP and stdio transport layers
- **OAuth 2.0 Authentication**: Complete OAuth 2.0 support including:
  - Authorization Code Flow with PKCE
  - Dynamic client registration (RFC7591)
  - Automatic token refresh
  - MCP-specific `resource` parameter support
  - Built-in callback server for browser flows
- **Async/Await**: Built on Tokio for high-performance async operations

**Note**: Batch operations in the previous protocol version are not supported.

---

## Example 

From `./crates/tenx-mcp/examples/weather_server.rs` 

```rust
use serde::{Deserialize, Serialize};
use tenx_mcp::{mcp_server, schema::*, schemars, tool, Result, Server, ServerCtx};

#[derive(Default)]
struct WeatherServer;

// Tool input schema is automatically derived from the struct using serde and schemars.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct WeatherParams {
    city: String,
}

// The mcp_server macro generates the necessary boilerplate to expose methods as MCP tools.
#[mcp_server]
impl WeatherServer {
    // The doc comment becomes the tool's description in the MCP schema.
    #[tool]
    /// Get current weather for a city
    async fn get_weather(&self, _ctx: &ServerCtx, params: WeatherParams) -> Result<CallToolResult> {
        // Simulate weather API call
        let temperature = 22.5;
        let conditions = "Partly cloudy";

        Ok(CallToolResult::new().with_text_content(format!(
            "Weather in {}: {}°C, {}",
            params.city, temperature, conditions
        )))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = Server::default().with_connection(WeatherServer::default);
    server.serve_stdio().await?;
    Ok(())
}
```
