![Discord](https://img.shields.io/discord/1381424110831145070?style=flat-square&logo=rust&link=https%3A%2F%2Fdiscord.gg%2FfHmRmuBDxF)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

# tenx-mcp

A Rust implementation of the Model Context Protocol (MCP) for building AI-integrated applications.

--- 

## Community

Want to contribute? Have ideas or feature requests? Come tell us about it on
[Discord](https://discord.gg/fHmRmuBDxF). 

---

## Overview

tenx-mcp provides both client and server capabilities for the Model Context
Protocol, enabling seamless communication between AI language models and
external tools/services. MCP is a standardized protocol that allows AI
assistants to interact with various tools, access resources, and provide
enhanced capabilities through a well-defined interface.

---

## Features

- **Full MCP Protocol Support**: Implements the latest MCP specification
  (2025-03-26) with optional support for the previous version
- **Client & Server**: Build both MCP clients and servers with ergonomic APIs
- **Multiple Transports**: TCP/IP and stdio transport layers
- **OAuth 2.0 Authentication**: Complete OAuth 2.0 support including:
  - Authorization Code Flow with PKCE
  - Dynamic client registration (RFC7591)
  - Automatic token refresh
  - MCP-specific `resource` parameter support
  - Built-in callback server for browser flows
- **Async/Await**: Built on Tokio for high-performance async operations
- **Type-Safe**: Leverages Rust's type system with comprehensive schema
  definitions
- **Extensible**: Easy to add custom handlers for tools, resources, and prompts

**Note**: Batch operations in the previous protocol version are not supported.

