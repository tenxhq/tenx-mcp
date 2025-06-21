//! Macros to make defining MCP servers easier
//!
//! Using the #[mcp_server] macro on an impl block, this crate will pick up all methods
//! marked with #[tool] and derive the necessary ServerConn::call_tool, ServerConn::list_tools and
//! ServerConn::initialize methods. The name of the server is derived from the name of the struct,
//! and the description is derived from the doc comment on the impl block. The version is set to
//! "0.1.0" by default.
//!
//! The the final argument to the tool method must be a struct that implements
//! `schemars::JsonSchema`.
//!
//! Example usage:
//!
//!  ```rust
//! #[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
//! struct EchoParams {
//!     /// The message to echo back
//!     message: String,
//! }
//!
//! /// Basic server connection that provides an echo tool
//! #[derive(Debug, Default)]
//! struct Basic {}
//!
//! #[mcp_server]
//! /// This is the description field for the server.
//! impl Basic {
//!     #[tool]
//!     async fn echo(
//!         &mut self,
//!         context: &ServerCtx,
//!         params: EchoParams,
//!     ) -> Result<schema::CallToolResult> {
//!         Ok(schema::CallToolResult::new().with_text_content(params.message))
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};
use tenx_mcp::{macros::*, schema, schemars, Result, ServerCtx};

const NAME: &str = "basic-server";
const VERSION: &str = "0.1.0";

/// Echo tool input parameters
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct EchoParams {
    /// The message to echo back
    message: String,
}

/// Basic server connection that provides an echo tool
#[derive(Debug, Default)]
struct Basic {}

#[mcp_server]
/// This is the description field for the server.
impl Basic {
    #[tool]
    async fn echo(
        &mut self,
        context: &ServerCtx,
        params: EchoParams,
    ) -> Result<schema::CallToolResult> {
        Ok(schema::CallToolResult::new().with_text_content(params.message))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    Ok(())
}
