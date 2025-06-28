use oauth2::{
    basic::{
        BasicClient, BasicErrorResponse, BasicRevocationErrorResponse,
        BasicTokenIntrospectionResponse, BasicTokenResponse,
    },
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, StandardRevocableToken,
    TokenResponse, TokenUrl,
};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use url::Url;

use super::dynamic_registration::{ClientMetadata, DynamicRegistrationClient};
use crate::error::Error;

#[derive(Debug, Clone)]
pub struct OAuth2Config {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_url: String,
    pub resource: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct OAuth2Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<std::time::Instant>,
}

type ConfiguredClient = oauth2::Client<
    BasicErrorResponse,
    BasicTokenResponse,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

pub struct OAuth2Client {
    client: ConfiguredClient,
    config: OAuth2Config,
    token: Arc<RwLock<Option<OAuth2Token>>>,
    refresh_lock: Arc<Mutex<()>>,
    pkce_verifier: Option<PkceCodeVerifier>,
    csrf_token: Option<CsrfToken>,
}

impl OAuth2Client {
    /// Perform dynamic client registration and create an OAuth2Client
    pub async fn register_dynamic(
        auth_url: String,
        token_url: String,
        resource: String,
        client_name: String,
        redirect_url: String,
        scopes: Vec<String>,
        registration_endpoint: Option<String>,
    ) -> Result<Self, Error> {
        let registration_client = DynamicRegistrationClient::new();

        // Try to discover registration endpoint if not provided
        let reg_endpoint = if let Some(endpoint) = registration_endpoint {
            endpoint
        } else {
            // Extract issuer from auth URL
            let auth_url_parsed = Url::parse(&auth_url)
                .map_err(|e| Error::InvalidConfiguration(format!("Invalid auth URL: {e}")))?;
            let issuer = format!(
                "{}://{}",
                auth_url_parsed.scheme(),
                auth_url_parsed.host_str().unwrap_or("")
            );

            // Try to discover registration endpoint
            match registration_client
                .discover_registration_endpoint(&issuer)
                .await
            {
                Ok(Some(endpoint)) => endpoint,
                Ok(None) => {
                    return Err(Error::InvalidConfiguration(
                        "No registration endpoint found in OAuth metadata".to_string(),
                    ));
                }
                Err(e) => return Err(e),
            }
        };

        // Create client metadata
        let metadata = ClientMetadata::new(&client_name, &redirect_url)
            .with_resource(&resource)
            .with_scopes(scopes.clone())
            .with_software_info("tenx-mcp", env!("CARGO_PKG_VERSION"));

        // Register the client
        let registration = registration_client
            .register(&reg_endpoint, metadata, None)
            .await?;

        // Create OAuth2Config from registration response
        let config = OAuth2Config::from_registration(registration, auth_url, token_url, resource);

        // Create the OAuth2Client
        Self::new(config)
    }

    pub fn new(config: OAuth2Config) -> Result<Self, Error> {
        let mut client = BasicClient::new(ClientId::new(config.client_id.clone()))
            .set_auth_uri(
                AuthUrl::new(config.auth_url.clone())
                    .map_err(|e| Error::InvalidConfiguration(format!("Invalid auth URL: {e}")))?,
            )
            .set_token_uri(
                TokenUrl::new(config.token_url.clone())
                    .map_err(|e| Error::InvalidConfiguration(format!("Invalid token URL: {e}")))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(config.redirect_url.clone()).map_err(|e| {
                    Error::InvalidConfiguration(format!("Invalid redirect URL: {e}"))
                })?,
            );

        if let Some(client_secret) = config.client_secret.as_ref() {
            client = client.set_client_secret(ClientSecret::new(client_secret.clone()));
        }

        // For now, we don't set revocation URL during construction to maintain type compatibility
        // The revocation URL can be set later if needed

        Ok(Self {
            client,
            config,
            token: Arc::new(RwLock::new(None)),
            refresh_lock: Arc::new(Mutex::new(())),
            pkce_verifier: None,
            csrf_token: None,
        })
    }

    pub fn get_authorization_url(&mut self) -> (Url, CsrfToken) {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        self.pkce_verifier = Some(pkce_verifier);

        let mut auth_request = self
            .client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge)
            .add_extra_param("resource", &self.config.resource);

        for scope in &self.config.scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }

        let (url, csrf_token) = auth_request.url();
        self.csrf_token = Some(csrf_token.clone());
        (url, csrf_token)
    }

    pub async fn exchange_code(
        &mut self,
        code: String,
        state: String,
    ) -> Result<OAuth2Token, Error> {
        let stored_token = self
            .csrf_token
            .take()
            .ok_or_else(|| Error::InvalidConfiguration("Missing CSRF token".to_string()))?;

        if state != *stored_token.secret() {
            return Err(Error::AuthorizationFailed("CSRF token mismatch".into()));
        }

        let pkce_verifier = self
            .pkce_verifier
            .take()
            .ok_or_else(|| Error::InvalidConfiguration("Missing PKCE verifier".to_string()))?;

        let token_result = self
            .client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .add_extra_param("resource", &self.config.resource)
            .request_async(&reqwest::Client::new())
            .await
            .map_err(|e| Error::AuthorizationFailed(format!("Token exchange failed: {e}")))?;

        let oauth_token = OAuth2Token {
            access_token: token_result.access_token().secret().clone(),
            refresh_token: token_result.refresh_token().map(|t| t.secret().clone()),
            expires_at: token_result
                .expires_in()
                .map(|duration| std::time::Instant::now() + duration),
        };

        *self.token.write().await = Some(oauth_token.clone());
        Ok(oauth_token)
    }

    pub async fn get_valid_token(&self) -> Result<String, Error> {
        let now = std::time::Instant::now();
        {
            let token_guard = self.token.read().await;
            if let Some(token) = &*token_guard {
                if token.expires_at.map(|exp| exp > now).unwrap_or(true) {
                    return Ok(token.access_token.clone());
                }
            }
        }

        let _refresh_guard = self.refresh_lock.lock().await;

        // Check again after obtaining the lock in case another task refreshed
        let refresh_token_opt = {
            let token_guard = self.token.read().await;
            if let Some(token) = &*token_guard {
                if token.expires_at.map(|exp| exp > now).unwrap_or(true) {
                    return Ok(token.access_token.clone());
                }
                token.refresh_token.clone()
            } else {
                None
            }
        };

        if let Some(refresh_token) = refresh_token_opt {
            self.refresh_token_inner(&refresh_token).await
        } else {
            Err(Error::AuthorizationFailed(
                "No valid token available".to_string(),
            ))
        }
    }
    async fn refresh_token_inner(&self, refresh_token: &str) -> Result<String, Error> {
        let token_result = self
            .client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
            .add_extra_param("resource", &self.config.resource)
            .request_async(&reqwest::Client::new())
            .await
            .map_err(|e| Error::AuthorizationFailed(format!("Token refresh failed: {e}")))?;

        let oauth_token = OAuth2Token {
            access_token: token_result.access_token().secret().clone(),
            refresh_token: token_result.refresh_token().map(|t| t.secret().clone()),
            expires_at: token_result
                .expires_in()
                .map(|duration| std::time::Instant::now() + duration),
        };

        let access_token = oauth_token.access_token.clone();
        *self.token.write().await = Some(oauth_token);
        Ok(access_token)
    }

    pub fn set_token(&self, token: OAuth2Token) -> impl std::future::Future<Output = ()> + Send {
        let token_arc = self.token.clone();
        async move {
            *token_arc.write().await = Some(token);
        }
    }
}

pub struct OAuth2CallbackServer {
    port: u16,
}

impl OAuth2CallbackServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    pub async fn wait_for_callback(&self) -> Result<(String, String), Error> {
        use tokio::net::TcpListener;

        let addr = format!("127.0.0.1:{}", self.port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| Error::TransportError(format!("Failed to bind to {addr}: {e}")))?;

        let (stream, _) = listener
            .accept()
            .await
            .map_err(|e| Error::TransportError(format!("Failed to accept connection: {e}")))?;

        const MAX_REQUEST_SIZE: usize = 8 * 1024; // 8 KiB
        const MAX_LINE_LENGTH: usize = 1024;

        let mut request = String::new();
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        let mut total_read = 0usize;
        loop {
            let mut line = String::new();
            let bytes = reader
                .read_line(&mut line)
                .await
                .map_err(|e| Error::TransportError(format!("Failed to read from socket: {e}")))?;

            if bytes == 0 {
                break;
            }

            if line.len() > MAX_LINE_LENGTH || total_read + line.len() > MAX_REQUEST_SIZE {
                return Err(Error::AuthorizationFailed("HTTP request too large".into()));
            }

            total_read += line.len();

            if line.trim().is_empty() {
                break;
            }
            request.push_str(&line);
        }

        let (code, state) = Self::extract_code_and_state(&request)?;

        let response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/html\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            SUCCESS_HTML.len(),
            SUCCESS_HTML
        );

        writer
            .write_all(response.as_bytes())
            .await
            .map_err(|e| Error::TransportError(format!("Failed to send response: {e}")))?;

        Ok((code, state))
    }

    fn extract_code_and_state(request: &str) -> Result<(String, String), Error> {
        let lines: Vec<&str> = request.lines().collect();
        if lines.is_empty() {
            return Err(Error::AuthorizationFailed(
                "Empty request received".to_string(),
            ));
        }

        let request_line = lines[0];
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 3 {
            return Err(Error::AuthorizationFailed(
                "Invalid HTTP request line".to_string(),
            ));
        }

        let method = parts[0];
        if method != "GET" {
            return Err(Error::AuthorizationFailed(format!(
                "Unsupported HTTP method: {method}"
            )));
        }

        let path = parts[1];
        if !path.starts_with("/callback") {
            return Err(Error::AuthorizationFailed("Invalid callback path".into()));
        }
        let url = Url::parse(&format!("http://localhost{path}")).map_err(|e| {
            Error::AuthorizationFailed(format!("Failed to parse callback URL: {e}"))
        })?;

        let query_pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();

        let code = query_pairs
            .get("code")
            .ok_or_else(|| Error::AuthorizationFailed("Missing authorization code".to_string()))?
            .to_string();

        let state = query_pairs
            .get("state")
            .ok_or_else(|| Error::AuthorizationFailed("Missing state parameter".to_string()))?
            .to_string();

        Ok((code, state))
    }
}

const SUCCESS_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <title>Authorization Successful</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            display: flex;
            justify-content: center;
            align-items: center;
            height: 100vh;
            margin: 0;
            background-color: #f5f5f5;
        }
        .container {
            text-align: center;
            padding: 2rem;
            background: white;
            border-radius: 8px;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }
        h1 { color: #22c55e; }
        p { color: #666; margin-top: 1rem; }
    </style>
</head>
<body>
    <div class="container">
        <h1>✓ Authorization Successful</h1>
        <p>You can now close this window and return to your terminal.</p>
    </div>
</body>
</html>"#;
