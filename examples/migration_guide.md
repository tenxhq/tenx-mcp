# Migration Guide: From Handler Traits to Connection Trait

This guide shows how to migrate from the legacy handler traits API to the new Connection trait API.

## Old API (Handler Traits)

Previously, you would create separate handlers for tools, resources, and prompts:

```rust
use tenx_mcp::{MCPServer, ToolHandler, ResourceHandler, PromptHandler};

// Define tool handlers
struct EchoToolHandler;

#[async_trait]
impl ToolHandler for EchoToolHandler {
    fn metadata(&self) -> Tool {
        // Return tool metadata
    }
    
    async fn execute(&self, arguments: Option<Value>) -> Result<Vec<Content>> {
        // Execute tool
    }
}

// Create server and register handlers
let mut server = MCPServer::new("my-server".to_string(), "1.0.0".to_string());
server.register_tool(Box::new(EchoToolHandler));
server.register_resource(Box::new(MyResourceHandler));
server.register_prompt(Box::new(MyPromptHandler));
```

## New API (Connection Trait)

With the new API, you implement a single Connection trait that handles all operations for a connection:

```rust
use tenx_mcp::{Connection, ConnectionContext, MCPServer};

struct MyConnection {
    // Connection-specific state
    request_count: u64,
    context: Option<ConnectionContext>,
}

#[async_trait]
impl Connection for MyConnection {
    async fn on_connect(&mut self, context: ConnectionContext) -> Result<()> {
        // Called when connection is established
        self.context = Some(context);
        Ok(())
    }

    async fn initialize(
        &mut self,
        protocol_version: String,
        capabilities: ClientCapabilities,
        client_info: Implementation,
    ) -> Result<InitializeResult> {
        // Handle initialization
    }

    async fn tools_list(&mut self) -> Result<ListToolsResult> {
        // Return available tools
    }

    async fn tools_call(&mut self, name: String, arguments: Option<Value>) -> Result<CallToolResult> {
        // Execute tool
    }

    // ... other methods with default implementations
}

// Create server with connection factory
let server = MCPServer::new("my-server".to_string(), "1.0.0".to_string())
    .with_connection_factory(|| Box::new(MyConnection::new()));
```

## Key Benefits

1. **Per-connection state**: Each connection gets its own instance, allowing you to track connection-specific state
2. **Unified interface**: All functionality is in one trait instead of multiple handler traits
3. **Better notifications**: Direct access to ConnectionContext for sending notifications
4. **Lifecycle hooks**: `on_connect` and `on_disconnect` methods for setup/cleanup

## Using Legacy Handlers with Connection Trait

You can still use the existing handler traits within your Connection implementation:

```rust
struct MyConnection {
    tools: HashMap<String, Box<dyn ToolHandler>>,
}

impl MyConnection {
    fn new() -> Self {
        let mut tools = HashMap::new();
        tools.insert("echo".to_string(), Box::new(EchoToolHandler));
        Self { tools }
    }
}

#[async_trait]
impl Connection for MyConnection {
    async fn tools_list(&mut self) -> Result<ListToolsResult> {
        let tool_list: Vec<Tool> = self.tools.values()
            .map(|h| h.metadata())
            .collect();
        
        Ok(ListToolsResult {
            tools: tool_list,
            // ...
        })
    }

    async fn tools_call(&mut self, name: String, arguments: Option<Value>) -> Result<CallToolResult> {
        match self.tools.get(&name) {
            Some(handler) => {
                let content = handler.execute(arguments).await?;
                Ok(CallToolResult {
                    content,
                    is_error: Some(false),
                    // ...
                })
            }
            None => Err(MCPError::ToolExecutionFailed {
                tool: name,
                message: "Tool not found".to_string(),
            }),
        }
    }
}
```

## Complete Example

See `examples/connection_server.rs` for a complete working example of the new Connection trait API.