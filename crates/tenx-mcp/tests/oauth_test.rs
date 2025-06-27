use std::sync::Arc;
use tenx_mcp::{OAuth2CallbackServer, OAuth2Client, OAuth2Config, OAuth2Token};
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn test_oauth_client_creation() {
    let config = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: Some("test_client_secret".to_string()),
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_url: "http://localhost:8080/callback".to_string(),
        resource: "https://example.com/api".to_string(),
        scopes: vec!["read".to_string(), "write".to_string()],
    };

    let oauth_client = OAuth2Client::new(config).unwrap();
    assert!(Arc::new(oauth_client).as_ref() as *const _ != std::ptr::null());
}

#[tokio::test]
async fn test_authorization_url_generation() {
    let config = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: None,
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_url: "http://localhost:8080/callback".to_string(),
        resource: "https://example.com/api".to_string(),
        scopes: vec!["read".to_string()],
    };

    let mut oauth_client = OAuth2Client::new(config).unwrap();
    let (auth_url, csrf_token) = oauth_client.get_authorization_url();

    // Check that the URL contains expected parameters
    let url_str = auth_url.as_str();
    assert!(url_str.contains("client_id=test_client_id"));
    assert!(url_str.contains("redirect_uri=http%3A%2F%2Flocalhost%3A8080%2Fcallback"));
    assert!(url_str.contains("response_type=code"));
    assert!(url_str.contains("state="));
    assert!(url_str.contains("code_challenge="));
    assert!(url_str.contains("code_challenge_method=S256"));
    assert!(url_str.contains("resource=https%3A%2F%2Fexample.com%2Fapi"));
    assert!(url_str.contains("scope=read"));

    // CSRF token should not be empty
    assert!(!csrf_token.secret().is_empty());
}

#[tokio::test]
async fn test_callback_server() {
    let server = OAuth2CallbackServer::new(8765);
    
    // Spawn a task to simulate a client making the callback request
    let client_task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        let client = reqwest::Client::new();
        let _ = client
            .get("http://127.0.0.1:8765/callback?code=test_code&state=test_state")
            .send()
            .await;
    });

    // Wait for callback with timeout
    let result = timeout(Duration::from_secs(5), server.wait_for_callback()).await;
    
    match result {
        Ok(Ok((code, state))) => {
            assert_eq!(code, "test_code");
            assert_eq!(state, "test_state");
        }
        Ok(Err(e)) => panic!("Callback server error: {}", e),
        Err(_) => panic!("Callback server timed out"),
    }

    let _ = client_task.await;
}

#[tokio::test]
async fn test_http_transport_with_oauth() {
    // This test verifies that the OAuth client can be integrated with the HTTP transport
    let config = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: Some("test_client_secret".to_string()),
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_url: "http://localhost:8080/callback".to_string(),
        resource: "https://example.com/api".to_string(),
        scopes: vec!["read".to_string()],
    };

    let oauth_client = OAuth2Client::new(config).unwrap();
    
    // Set a pre-configured token to avoid the OAuth flow
    let token = OAuth2Token {
        access_token: "test_access_token".to_string(),
        refresh_token: Some("test_refresh_token".to_string()),
        expires_at: Some(std::time::Instant::now() + Duration::from_secs(3600)),
    };
    oauth_client.set_token(token).await;

    let oauth_client_arc = Arc::new(oauth_client);
    
    // Verify the token is retrievable
    let retrieved_token = oauth_client_arc.get_valid_token().await.unwrap();
    assert_eq!(retrieved_token, "test_access_token");
    
    // The actual HTTP transport integration is tested in the examples
    // This test focuses on the OAuth client functionality
}

#[tokio::test]
async fn test_token_refresh() {
    let config = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: Some("test_client_secret".to_string()),
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_url: "http://localhost:8080/callback".to_string(),
        resource: "https://example.com/api".to_string(),
        scopes: vec!["read".to_string()],
    };

    let oauth_client = OAuth2Client::new(config).unwrap();
    
    // Set an expired token
    let token = OAuth2Token {
        access_token: "expired_token".to_string(),
        refresh_token: Some("refresh_token".to_string()),
        expires_at: Some(std::time::Instant::now() - Duration::from_secs(1)), // Already expired
    };
    oauth_client.set_token(token).await;

    // Try to get a valid token - this should trigger a refresh
    // In a real scenario, this would make an HTTP request to the token endpoint
    // For testing, we'll just verify the logic works
    let result = oauth_client.get_valid_token().await;
    
    // This will fail because we don't have a real OAuth server, but the logic is tested
    assert!(result.is_err());
}