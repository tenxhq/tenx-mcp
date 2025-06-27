use clap::Parser;
use std::sync::Arc;
use tenx_mcp::{Client, OAuth2CallbackServer, OAuth2Client, OAuth2Config, OAuth2Token, ServerAPI};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(author, version, about = "Connect to GitHub MCP server with OAuth", long_about = None)]
struct Args {
    /// GitHub OAuth App Client ID
    #[arg(short, long, default_value = "")]
    client_id: String,

    /// GitHub OAuth App Client Secret
    #[arg(short = 's', long, default_value = "")]
    client_secret: String,

    /// OAuth callback port (default: 8080)
    #[arg(short = 'p', long, default_value = "8080")]
    port: u16,

    /// Use existing access token (skip OAuth flow)
    #[arg(short = 't', long)]
    access_token: Option<String>,
}

const GITHUB_MCP_ENDPOINT: &str = "https://api.githubcopilot.com/mcp/x/users/readonly";
const GITHUB_AUTH_URL: &str = "https://github.com/login/oauth/authorize";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set up logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let mut args = Args::parse();

    // Try to load from environment variables if not provided via CLI
    if args.client_id.is_empty() {
        if let Ok(client_id) = std::env::var("GITHUB_CLIENT_ID") {
            args.client_id = client_id;
        }
    }

    if args.client_secret.is_empty() {
        if let Ok(client_secret) = std::env::var("GITHUB_CLIENT_SECRET") {
            args.client_secret = client_secret;
        }
    }

    if args.access_token.is_none() {
        if let Ok(access_token) = std::env::var("GITHUB_ACCESS_TOKEN") {
            args.access_token = Some(access_token);
        }
    }

    // Validate required arguments
    if args.client_id.is_empty() || args.client_secret.is_empty() {
        eprintln!("Error: GitHub Client ID and Client Secret are required");
        eprintln!("Provide them via command line arguments or environment variables:");
        eprintln!("  export GITHUB_CLIENT_ID=your_client_id");
        eprintln!("  export GITHUB_CLIENT_SECRET=your_client_secret");
        std::process::exit(1);
    }

    // Create OAuth client
    let oauth_client = if let Some(access_token) = args.access_token {
        // Use provided access token
        info!("Using provided access token");

        let config = OAuth2Config {
            client_id: args.client_id,
            client_secret: Some(args.client_secret),
            auth_url: GITHUB_AUTH_URL.to_string(),
            token_url: GITHUB_TOKEN_URL.to_string(),
            redirect_url: format!("http://localhost:{}/callback", args.port),
            resource: GITHUB_MCP_ENDPOINT.to_string(),
            scopes: vec!["read:user".to_string()], // GitHub scope for reading user data
        };

        let oauth_client = OAuth2Client::new(config)?;

        // Set the provided token
        let token = OAuth2Token {
            access_token,
            refresh_token: None,
            expires_at: None, // GitHub tokens don't expire by default
        };
        oauth_client.set_token(token).await;

        oauth_client
    } else {
        // Perform full OAuth flow
        info!("Starting OAuth flow...");

        let config = OAuth2Config {
            client_id: args.client_id,
            client_secret: Some(args.client_secret),
            auth_url: GITHUB_AUTH_URL.to_string(),
            token_url: GITHUB_TOKEN_URL.to_string(),
            redirect_url: format!("http://localhost:{}/callback", args.port),
            resource: GITHUB_MCP_ENDPOINT.to_string(),
            scopes: vec!["read:user".to_string()],
        };

        let mut oauth_client = OAuth2Client::new(config)?;

        // Get authorization URL
        let (auth_url, csrf_token) = oauth_client.get_authorization_url();

        info!("Opening browser for GitHub authorization...");
        info!("If the browser doesn't open, visit: {}", auth_url);

        // Try to open the browser
        if let Err(e) = webbrowser::open(auth_url.as_str()) {
            eprintln!("Failed to open browser: {e}");
            eprintln!("Please visit the URL manually: {auth_url}");
        }

        // Start callback server
        let callback_server = OAuth2CallbackServer::new(args.port);
        info!("Waiting for OAuth callback on port {}...", args.port);

        let (code, state) = callback_server.wait_for_callback().await?;

        // Verify CSRF token
        if state != *csrf_token.secret() {
            return Err("CSRF token mismatch".into());
        }

        info!("Received authorization code, exchanging for token...");

        // Exchange code for token
        let token = oauth_client.exchange_code(code, state).await?;
        info!("Successfully obtained access token: {}", token.access_token);

        oauth_client
    };

    // Create MCP client with OAuth
    let oauth_client_arc = Arc::new(oauth_client);
    let mut client = Client::new("ghusers-example", "1.0.0");

    info!(
        "Connecting to GitHub MCP server at {}...",
        GITHUB_MCP_ENDPOINT
    );

    let init_result = client
        .connect_http_with_oauth(GITHUB_MCP_ENDPOINT, oauth_client_arc.clone())
        .await?;

    info!("Connected successfully!");
    info!(
        "Server info: {} v{}",
        init_result.server_info.name, init_result.server_info.version
    );

    // List available tools
    info!("\n=== Available Tools ===");
    let tools = client.list_tools(None).await?;

    if tools.tools.is_empty() {
        info!("No tools available");
    } else {
        for tool in &tools.tools {
            info!("\nTool: {}", tool.name);
            if let Some(desc) = &tool.description {
                info!("  Description: {}", desc);
            }
            info!(
                "  Input schema: {}",
                serde_json::to_string_pretty(&tool.input_schema)?
            );
            if let Some(output_schema) = &tool.output_schema {
                info!(
                    "  Output schema: {}",
                    serde_json::to_string_pretty(output_schema)?
                );
            }
        }
    }

    // List available resources
    info!("\n=== Available Resources ===");
    let resources = client.list_resources(None).await?;

    if resources.resources.is_empty() {
        info!("No resources available");
    } else {
        for resource in &resources.resources {
            info!("\nResource: {}", resource.uri);
            if let Some(desc) = &resource.description {
                info!("  Description: {}", desc);
            }
            if let Some(mime_type) = &resource.mime_type {
                info!("  MIME type: {}", mime_type);
            }
        }
    }

    // List available prompts
    info!("\n=== Available Prompts ===");
    let prompts = client.list_prompts(None).await?;

    if prompts.prompts.is_empty() {
        info!("No prompts available");
    } else {
        for prompt in &prompts.prompts {
            info!("\nPrompt: {}", prompt.name);
            if let Some(desc) = &prompt.description {
                info!("  Description: {}", desc);
            }
            if let Some(arguments) = &prompt.arguments {
                if !arguments.is_empty() {
                    info!("  Arguments:");
                    for arg in arguments {
                        info!(
                            "    - {}: {} (required: {})",
                            arg.name,
                            arg.description.as_ref().unwrap_or(&"".to_string()),
                            arg.required.unwrap_or(false)
                        );
                    }
                }
            }
        }
    }

    // Get the access token for future use
    if let Ok(token) = oauth_client_arc.get_valid_token().await {
        info!("\n=== Authentication Info ===");
        info!("Access token (save for future use): {}", token);
        info!(
            "You can skip the OAuth flow next time by using: --access-token {}",
            token
        );
    }

    Ok(())
}
