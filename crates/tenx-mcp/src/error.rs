use crate::schema::{
    ErrorObject, INVALID_PARAMS, INVALID_REQUEST, JSONRPC_VERSION, JSONRPCError, METHOD_NOT_FOUND,
    PARSE_ERROR, RequestId,
};
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum Error {
    #[error("IO error: {message}")]
    Io { message: String },

    #[error("JSON serialization error: {message}")]
    JsonParse { message: String },

    #[error("Transport error: {0}")]
    Transport(String),

    #[error("Transport disconnected unexpectedly")]
    TransportDisconnected,

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

    #[error("Handler error for {handler_type}: {message}")]
    HandlerError {
        handler_type: String,
        message: String,
    },

    #[error("Resource not found: {uri}")]
    ResourceNotFound { uri: String },

    #[error("Tool execution failed for '{tool}': {message}")]
    ToolExecutionFailed { tool: String, message: String },

    #[error("Invalid message format: {message}")]
    InvalidMessageFormat { message: String },

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Authorization failed: {0}")]
    AuthorizationFailed(String),

    #[error("Transport error: {0}")]
    TransportError(String),
}

impl Error {
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

    /// Convert error to a specific JSONRPC response if applicable
    pub(crate) fn to_jsonrpc_response(&self, request_id: RequestId) -> Option<JSONRPCError> {
        let (code, message) = match self {
            Self::ToolNotFound(tool_name) => {
                (METHOD_NOT_FOUND, format!("Tool not found: {tool_name}"))
            }
            Self::MethodNotFound(method_name) => {
                (METHOD_NOT_FOUND, format!("Method not found: {method_name}"))
            }
            Self::InvalidParams(message) => {
                (INVALID_PARAMS, format!("Invalid parameters: {message}"))
            }
            Self::InvalidRequest(msg) => (INVALID_REQUEST, format!("Invalid request: {msg}")),
            Self::JsonParse { message } => {
                (PARSE_ERROR, format!("JSON serialization error: {message}"))
            }
            Self::InvalidMessageFormat { message } => {
                (PARSE_ERROR, format!("Invalid message format: {message}"))
            }
            // Return None for errors that should use the generic INTERNAL_ERROR handling
            _ => return None,
        };

        Some(JSONRPCError {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id: request_id,
            error: ErrorObject {
                code,
                message,
                data: None,
            },
        })
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Io {
            message: err.to_string(),
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonParse {
            message: err.to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;
