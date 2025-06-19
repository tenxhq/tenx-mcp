use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tenx_mcp::{schema::*, ClientConnection, ClientConnectionContext, Result};

/// Test client connection that tracks method calls
#[derive(Default)]
struct TestClientConnection {
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ClientConnection for TestClientConnection {
    async fn on_connect(&mut self, _context: ClientConnectionContext) -> Result<()> {
        self.calls.lock().unwrap().push("on_connect".to_string());
        Ok(())
    }

    async fn on_disconnect(&mut self, _context: ClientConnectionContext) -> Result<()> {
        self.calls.lock().unwrap().push("on_disconnect".to_string());
        Ok(())
    }

    async fn ping(&mut self, _context: ClientConnectionContext) -> Result<()> {
        self.calls.lock().unwrap().push("ping".to_string());
        Ok(())
    }

    async fn create_message(
        &mut self,
        _context: ClientConnectionContext,
        _method: &str,
        _params: CreateMessageParams,
    ) -> Result<CreateMessageResult> {
        self.calls
            .lock()
            .unwrap()
            .push("create_message".to_string());
        Ok(CreateMessageResult {
            role: Role::Assistant,
            content: SamplingContent::Text(TextContent {
                text: "Test response".to_string(),
                annotations: None,
            }),
            model: "test-model".to_string(),
            stop_reason: None,
            meta: None,
        })
    }

    async fn list_roots(&mut self, _context: ClientConnectionContext) -> Result<ListRootsResult> {
        self.calls.lock().unwrap().push("list_roots".to_string());
        Ok(ListRootsResult {
            roots: vec![Root {
                uri: "test://root".to_string(),
                name: Some("Test Root".to_string()),
            }],
            meta: None,
        })
    }
}

#[tokio::test]
async fn test_client_connection_trait_methods() {
    // Test that the trait methods can be called
    let mut connection = TestClientConnection::default();

    // Create a dummy context for testing
    let (notification_tx, _) = tokio::sync::broadcast::channel(10);
    let context = ClientConnectionContext::new(notification_tx);

    // Test ping
    connection.ping(context.clone()).await.expect("Ping failed");

    // Test create_message
    let params = CreateMessageParams {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: SamplingContent::Text(TextContent {
                text: "Hello".to_string(),
                annotations: None,
            }),
        }],
        system_prompt: None,
        include_context: None,
        temperature: None,
        max_tokens: 1000,
        metadata: None,
        stop_sequences: None,
        model_preferences: None,
    };

    let result = connection
        .create_message(context.clone(), "test", params)
        .await
        .expect("Create message failed");
    assert_eq!(result.model, "test-model");

    // Test list_roots
    let roots = connection.list_roots(context.clone()).await.expect("List roots failed");
    assert_eq!(roots.roots.len(), 1);
    assert_eq!(roots.roots[0].uri, "test://root");

    // Verify all methods were called
    let calls = connection.calls.lock().unwrap();
    assert!(calls.contains(&"ping".to_string()));
    assert!(calls.contains(&"create_message".to_string()));
    assert!(calls.contains(&"list_roots".to_string()));
}
