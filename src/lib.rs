pub mod client;
pub mod codec;
pub mod error;
pub mod retry;
pub mod schema;
pub mod server;
pub mod transport;

// Re-export commonly used types
pub use client::MCPClient;
pub use error::{MCPError, Result};
pub use schema::{
    ClientCapabilities, Content, Implementation, JSONRPCNotification, ResourceContents,
    ToolsCapability,
};
pub use server::{MCPServer, MCPServerHandle, PromptHandler, ResourceHandler, ToolHandler};
pub use transport::{StdioTransport, TcpServerTransport, TcpTransport};

#[cfg(test)]
mod tests {
    use super::schema::*;

    #[test]
    fn test_jsonrpc_request_serialization() {
        let request = JSONRPCRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: RequestId::Number(1),
            request: Request {
                method: "initialize".to_string(),
                params: None,
            },
        };

        let json = serde_json::to_string(&request).unwrap();
        let parsed: JSONRPCRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.jsonrpc, JSONRPC_VERSION);
        assert_eq!(parsed.id, RequestId::Number(1));
        assert_eq!(parsed.request.method, "initialize");
    }

    #[test]
    fn test_role_serialization() {
        let role = Role::User;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"user\"");

        let role = Role::Assistant;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"assistant\"");
    }
}
