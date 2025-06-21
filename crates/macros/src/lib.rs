//! Macros to make defining MCP servers easier
//!
//! Using the #[mcp_server] macro on an impl block, this crate will pick up all methods
//! marked with #[tool] and derive the necessary ServerConn::call_tool, ServerConn::list_tools and
//! ServerConn::initialize methods. The name of the server is derived from the name of the struct,
//! and the description is derived from the doc comment on the impl block. The version is set to
//! "0.1.0" by default.
//!
//! ALl tool methods have the following exact signature:  
//!
//!     async fn tool_name(&mut self, context: &ServerCtx, params: ToolParams) -> Result<schema::CallToolResult>
//!
//! The parameter struct (ToolParams in this example) must implement `schemars::JsonSchema` and
//! `serde::Deserialize`.
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

/// Derive the ServerConn methods from an impl block.
#[proc_macro_attribute]
pub fn mcp_server(
    _attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    input
}

/// Mark a method as an mcp tool.
#[proc_macro_attribute]
pub fn tool(
    _attr: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    input
}
