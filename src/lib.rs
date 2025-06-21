mod api;
mod client;
mod client_connection;
mod codec;
mod error;
mod jsonrpc;
mod request_handler;
mod server;
mod server_connection;
mod transport;

pub mod schema;
pub mod testutils;

pub use api::*;
pub use client::Client;
pub use client_connection::{ClientConn, ClientCtx};
pub use error::{Error, Result};
pub use server::{Server, ServerCtx, ServerHandle};
pub use server_connection::ServerConn;

// Re-export schemars for users
pub use schemars;

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
