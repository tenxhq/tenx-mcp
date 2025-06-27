use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListResourcesResult {
    pub resources: Vec<Resource>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

impl ListResourcesResult {
    pub fn new() -> Self {
        Self {
            resources: Vec::new(),
            next_cursor: None,
        }
    }

    pub fn with_resource(mut self, resource: Resource) -> Self {
        self.resources.push(resource);
        self
    }

    pub fn with_resources(mut self, resources: impl IntoIterator<Item = Resource>) -> Self {
        self.resources.extend(resources);
        self
    }

    pub fn with_cursor(mut self, cursor: impl Into<Cursor>) -> Self {
        self.next_cursor = Some(cursor.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourceTemplatesResult {
    #[serde(rename = "resourceTemplates")]
    pub resource_templates: Vec<ResourceTemplate>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContents>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

impl ReadResourceResult {
    pub fn new() -> Self {
        Self {
            contents: Vec::new(),
            _meta: None,
        }
    }

    pub fn with_content(mut self, content: ResourceContents) -> Self {
        self.contents.push(content);
        self
    }

    pub fn with_contents(mut self, contents: impl IntoIterator<Item = ResourceContents>) -> Self {
        self.contents.extend(contents);
        self
    }

    pub fn with_meta(mut self, meta: HashMap<String, Value>) -> Self {
        self._meta = Some(meta);
        self
    }

    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        self._meta
            .get_or_insert_with(HashMap::new)
            .insert(key.into(), value);
        self
    }
}

/// A known resource that the server is capable of reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    /// Intended for programmatic or logical use, but used as a display name in past specs or fallback (if title isn't present).
    pub name: String,
    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    ///
    /// If not provided, the name should be used for display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

/// A template description for resources available on the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTemplate {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// Intended for programmatic or logical use, but used as a display name in past specs or fallback (if title isn't present).
    pub name: String,
    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    ///
    /// If not provided, the name should be used for display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResourceContents {
    Text(TextResourceContents),
    Blob(BlobResourceContents),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextResourceContents {
    pub uri: String,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobResourceContents {
    pub uri: String,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub blob: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}
