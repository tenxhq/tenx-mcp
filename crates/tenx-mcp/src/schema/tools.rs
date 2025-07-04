use super::*;
use crate::macros::{with_basename, with_meta};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// The server's response to a tools/list request from the client.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListToolsResult {
    pub tools: Vec<Tool>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

impl ListToolsResult {
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            next_cursor: None,
        }
    }

    pub fn with_tool(mut self, tool: Tool) -> Self {
        self.tools.push(tool);
        self
    }

    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Tool>) -> Self {
        self.tools.extend(tools);
        self
    }

    pub fn with_cursor(mut self, cursor: impl Into<Cursor>) -> Self {
        self.next_cursor = Some(cursor.into());
        self
    }
}

/// The server's response to a tool call.
#[with_meta]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    pub content: Vec<Content>,
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
}

impl CallToolResult {
    pub fn new() -> Self {
        Self {
            content: Vec::new(),
            is_error: None,
            structured_content: None,
            _meta: None,
        }
    }

    pub fn with_content(mut self, content: Content) -> Self {
        self.content.push(content);
        self
    }

    pub fn with_text_content(mut self, text: impl Into<String>) -> Self {
        self.content.push(Content::Text(TextContent {
            text: text.into(),
            annotations: None,
            _meta: None,
        }));
        self
    }

    pub fn is_error(mut self, is_error: bool) -> Self {
        self.is_error = Some(is_error);
        self
    }

    pub fn with_structured_content(mut self, content: Value) -> Self {
        self.structured_content = Some(content);
        self
    }
}

impl Default for CallToolResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Additional properties describing a Tool to clients.
///
/// NOTE: all properties in ToolAnnotations are hints.
/// They are not guaranteed to provide a faithful description of
/// tool behavior (including descriptive properties like `title`).
///
/// Clients should never make tool use decisions based on ToolAnnotations
/// received from untrusted servers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAnnotations {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

/// Definition for a tool the client can call.
#[with_meta]
#[with_basename]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    #[serde(rename = "inputSchema")]
    pub input_schema: ToolSchema,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<ToolSchema>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

impl Tool {
    pub fn new(name: impl Into<String>, input_schema: ToolSchema) -> Self {
        Self {
            input_schema,
            description: None,
            output_schema: None,
            annotations: None,
            name: name.into(),
            title: None,
            _meta: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_output_schema(mut self, schema: ToolSchema) -> Self {
        self.output_schema = Some(schema);
        self
    }

    pub fn with_annotation_title(mut self, title: impl Into<String>) -> Self {
        self.annotations.get_or_insert_with(Default::default).title = Some(title.into());
        self
    }

    pub fn with_read_only_hint(mut self, read_only: bool) -> Self {
        self.annotations
            .get_or_insert_with(Default::default)
            .read_only_hint = Some(read_only);
        self
    }

    pub fn with_destructive_hint(mut self, destructive: bool) -> Self {
        self.annotations
            .get_or_insert_with(Default::default)
            .destructive_hint = Some(destructive);
        self
    }

    pub fn with_idempotent_hint(mut self, idempotent: bool) -> Self {
        self.annotations
            .get_or_insert_with(Default::default)
            .idempotent_hint = Some(idempotent);
        self
    }

    pub fn with_open_world_hint(mut self, open_world: bool) -> Self {
        self.annotations
            .get_or_insert_with(Default::default)
            .open_world_hint = Some(open_world);
        self
    }

    pub fn with_annotations(mut self, annotations: ToolAnnotations) -> Self {
        self.annotations = Some(annotations);
        self
    }
}

/// A JSON Schema object defining the input or output schema for a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

impl Default for ToolSchema {
    fn default() -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: None,
            required: None,
        }
    }
}

impl ToolSchema {
    pub fn with_property(mut self, name: impl Into<String>, schema: Value) -> Self {
        self.properties
            .get_or_insert_with(HashMap::new)
            .insert(name.into(), schema);
        self
    }

    pub fn with_properties(mut self, properties: HashMap<String, Value>) -> Self {
        self.properties = Some(properties);
        self
    }

    pub fn with_required(mut self, name: impl Into<String>) -> Self {
        self.required.get_or_insert_with(Vec::new).push(name.into());
        self
    }

    pub fn with_required_properties(
        mut self,
        names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let required = self.required.get_or_insert_with(Vec::new);
        required.extend(names.into_iter().map(|n| n.into()));
        self
    }

    pub fn from_json_schema<T: schemars::JsonSchema>() -> Self {
        let schema = schemars::schema_for!(T);
        let schema_value = schema.as_value();
        let schema_obj = schema_value.as_object();
        let schema_type = schema_obj
            .and_then(|obj| obj.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("object")
            .to_string();
        let properties = schema_obj
            .and_then(|obj| obj.get("properties"))
            .and_then(|v| v.as_object())
            .map(|props| {
                props
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<HashMap<_, _>>()
            });
        let required = schema_obj
            .and_then(|obj| obj.get("required"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            });
        Self {
            schema_type,
            properties,
            required,
        }
    }

    /// Checks if a given property name is required in this schema.
    pub fn is_required(&self, name: &str) -> bool {
        self.required
            .as_ref()
            .map(|req| req.iter().any(|r| r == name))
            .unwrap_or(false)
    }
}
