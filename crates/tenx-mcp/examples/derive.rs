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
