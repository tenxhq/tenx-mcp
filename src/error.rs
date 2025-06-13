use thiserror::Error;

#[derive(Error, Debug)]
pub enum MCPError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Method not found: {0}")]
    MethodNotFound(String),

    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Request timeout")]
    Timeout,

    #[error("Request with id {0} not found")]
    RequestNotFound(String),
}

pub type Result<T> = std::result::Result<T, MCPError>;
