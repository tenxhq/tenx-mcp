use serde::{Deserialize, Serialize};
use tenx_mcp::{macros::*, schema, schemars, Result, ServerCtx};

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
        &self,
        _context: &ServerCtx,
        params: EchoParams,
    ) -> Result<schema::CallToolResult> {
        Ok(schema::CallToolResult::new().with_text_content(params.message))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // This example demonstrates the macro derive functionality.
    // The Basic struct now implements ServerConn trait automatically.
    let _server = Basic::default();

    // In a real application, you would use tenx_mcp::Server to serve this connection:
    // let server = tenx_mcp::Server::default()
    //     .with_connection_factory(|| Box::new(Basic::default()));

    println!("Server with echo tool created successfully!");
    Ok(())
}
