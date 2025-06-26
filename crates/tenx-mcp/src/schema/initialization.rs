use std::collections::HashMap;
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// After receiving an initialize request from the client, the server sends this response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// The version of the Model Context Protocol that the server wants to use.
    /// This may not match the version that the client requested. If the
    /// client cannot support this version, it MUST disconnect.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo")]
    pub server_info: Implementation,
    /// Instructions describing how to use the server and its features.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata
    /// to their responses.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

impl InitializeResult {
    /// Create a new InitializeResult with the latest protocol version
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities::default(),
            server_info: Implementation::new(name, version),
            instructions: None,
            meta: None,
        }
    }

    /// Set the instructions for the server
    pub fn with_instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Set the server capabilities
    pub fn with_capabilities(mut self, capabilities: ServerCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Enable logging capability
    pub fn with_logging(mut self) -> Self {
        self.capabilities.logging = Some(serde_json::Value::Object(serde_json::Map::new()));
        self
    }

    /// Enable prompts capability
    pub fn with_prompts(mut self, list_changed: bool) -> Self {
        self.capabilities.prompts = Some(PromptsCapability {
            list_changed: Some(list_changed),
        });
        self
    }

    /// Enable resources capability
    pub fn with_resources(mut self, subscribe: bool, list_changed: bool) -> Self {
        self.capabilities.resources = Some(ResourcesCapability {
            subscribe: Some(subscribe),
            list_changed: Some(list_changed),
        });
        self
    }

    /// Enable tools capability
    pub fn with_tools(mut self, list_changed: bool) -> Self {
        self.capabilities.tools = Some(ToolsCapability {
            list_changed: Some(list_changed),
        });
        self
    }

    /// Enable completions capability
    pub fn with_completions(mut self) -> Self {
        self.capabilities.completions = Some(serde_json::Value::Object(serde_json::Map::new()));
        self
    }
}
