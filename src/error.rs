use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum MCPError {
    #[error("IO error: {message}")]
    Io { message: String },

    #[error("JSON serialization error: {message}")]
    Json { message: String },

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Transport connection failed: {message}")]
    TransportConnectionFailed { message: String },

    #[error("Transport disconnected unexpectedly")]
    TransportDisconnected,

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Method not found: {0}")]
    MethodNotFound(String),

    #[error("Invalid parameters for method '{method}': {message}")]
    InvalidParams { method: String, message: String },

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Request timeout after {duration:?} for request id: {request_id}")]
    Timeout {
        duration: Duration,
        request_id: String,
    },

    #[error("Request with id {0} not found")]
    RequestNotFound(String),

    #[error("Handler error for {handler_type}: {message}")]
    HandlerError {
        handler_type: String,
        message: String,
    },

    #[error("Resource not found: {uri}")]
    ResourceNotFound { uri: String },

    #[error("Tool execution failed for '{tool}': {message}")]
    ToolExecutionFailed { tool: String, message: String },

    #[error("Message too large: {size} bytes exceeds maximum of {max_size} bytes")]
    MessageTooLarge { size: usize, max_size: usize },

    #[error("Invalid message format: {message}")]
    InvalidMessageFormat { message: String },

    #[error("Codec error: {message}")]
    CodecError { message: String },

    #[error("Tool not found: {0}")]
    ToolNotFound(String),
}

impl MCPError {
    /// Create an InvalidParams error with method context
    pub fn invalid_params(method: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidParams {
            method: method.into(),
            message: message.into(),
        }
    }

    /// Create a Timeout error with duration and request context
    pub fn timeout(duration: Duration, request_id: impl Into<String>) -> Self {
        Self::Timeout {
            duration,
            request_id: request_id.into(),
        }
    }

    /// Create a HandlerError with type context
    pub fn handler_error(handler_type: impl Into<String>, message: impl Into<String>) -> Self {
        Self::HandlerError {
            handler_type: handler_type.into(),
            message: message.into(),
        }
    }

    /// Create a ToolExecutionFailed error
    pub fn tool_execution_failed(tool: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ToolExecutionFailed {
            tool: tool.into(),
            message: message.into(),
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Io { .. }
                | Self::TransportConnectionFailed { .. }
                | Self::TransportDisconnected
                | Self::ConnectionClosed
                | Self::Timeout { .. }
        )
    }
}

impl From<std::io::Error> for MCPError {
    fn from(err: std::io::Error) -> Self {
        Self::Io {
            message: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for MCPError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json {
            message: err.to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, MCPError>;
