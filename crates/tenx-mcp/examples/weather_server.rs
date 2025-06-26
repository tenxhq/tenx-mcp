use serde::{Deserialize, Serialize};
use tenx_mcp::{macros::*, schema::*, schemars, Result, Server, ServerCtx};

#[derive(Default)]
struct WeatherServer;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
struct WeatherParams {
    city: String,
}

#[mcp_server]
impl WeatherServer {
    #[tool]
    /// Get current weather for a city
    async fn get_weather(&self, _ctx: &ServerCtx, params: WeatherParams) -> Result<CallToolResult> {
        // Simulate weather API call
        let temperature = 22.5;
        let conditions = "Partly cloudy";

        Ok(CallToolResult::new()
            .with_text_content(format!(
                "Weather in {}: {}°C, {}",
                params.city, temperature, conditions
            ))
            .is_error(false))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let server = Server::default().with_connection(WeatherServer::default);

    // Start server on HTTP port 3000
    let handle = server.serve_http("127.0.0.1:3000").await?;

    // Server runs until handle is dropped or explicitly aborted
    handle.handle.await.unwrap();
    Ok(())
}
