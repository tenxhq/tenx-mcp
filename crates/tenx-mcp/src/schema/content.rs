use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    Text(TextContent),
    Image(ImageContent),
    Audio(AudioContent),
    Resource(EmbeddedResource),
    #[serde(rename = "resourceLink")]
    ResourceLink(ResourceLink),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedResource {
    pub resource: ResourceContents,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotations {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<Role>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    #[serde(rename = "lastModified", skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextContent {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContent {
    pub data: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioContent {
    pub data: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}
