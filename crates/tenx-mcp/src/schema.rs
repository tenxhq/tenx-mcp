use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::request_handler::RequestMethod;

pub const PREVIOUS_PROTOCOL_VERSION: &str = "2025-03-26";
pub const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";
pub(crate) const JSONRPC_VERSION: &str = "2.0";

/// Refers to any valid JSON-RPC object that can be decoded off the wire, or
/// encoded to be sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JSONRPCMessage {
    Request(JSONRPCRequest),
    Notification(JSONRPCNotification),
    BatchRequest(JSONRPCBatchRequest),
    Response(JSONRPCResponse),
    Error(JSONRPCError),
    BatchResponse(JSONRPCBatchResponse),
}

/// A JSON-RPC batch request, as described in https://www.jsonrpc.org/specification#batch.
pub type JSONRPCBatchRequest = Vec<JSONRPCRequestOrNotification>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JSONRPCRequestOrNotification {
    Request(JSONRPCRequest),
    Notification(JSONRPCNotification),
}

/// A JSON-RPC batch response, as described in https://www.jsonrpc.org/specification#batch.
pub type JSONRPCBatchResponse = Vec<JSONRPCResponseOrError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JSONRPCResponseOrError {
    Response(JSONRPCResponse),
    Error(JSONRPCError),
}

/// A progress token, used to associate progress notifications with the original
/// request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProgressToken {
    String(String),
    Number(i64),
}

/// An opaque token used to represent a cursor for pagination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct Cursor(pub String);

impl From<&str> for Cursor {
    fn from(s: &str) -> Self {
        Cursor(s.to_string())
    }
}

impl From<String> for Cursor {
    fn from(s: String) -> Self {
        Cursor(s)
    }
}

impl From<&String> for Cursor {
    fn from(s: &String) -> Self {
        Cursor(s.clone())
    }
}

impl std::fmt::Display for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<RequestParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestParams {
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
    #[serde(flatten)]
    pub other: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMeta {
    /// If specified, the caller is requesting out-of-band progress
    /// notifications for this request (as represented by
    /// notifications/progress). The value of this parameter is an opaque token
    /// that will be attached to any subsequent notifications. The receiver is
    /// not obligated to provide these notifications.
    #[serde(rename = "progressToken", skip_serializing_if = "Option::is_none")]
    pub progress_token: Option<ProgressToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<NotificationParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationParams {
    /// This parameter name is reserved by MCP to allow clients and servers to
    /// attach additional metadata to their notifications.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
    #[serde(flatten)]
    pub other: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Result {
    /// This result property is reserved by the protocol to allow clients and
    /// servers to attach additional metadata to their responses.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
    #[serde(flatten)]
    pub other: HashMap<String, Value>,
}

/// A uniquely identifying ID for a request in JSON-RPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(i64),
}

/// A request that expects a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCRequest {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(flatten)]
    pub request: Request,
}

/// A notification which does not expect a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCNotification {
    pub jsonrpc: String,
    #[serde(flatten)]
    pub notification: Notification,
}

/// A successful (non-error) response to a request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    pub result: Result,
}

// Standard JSON-RPC error codes
pub(crate) const PARSE_ERROR: i32 = -32700;
pub(crate) const INVALID_REQUEST: i32 = -32600;
pub(crate) const METHOD_NOT_FOUND: i32 = -32601;
pub(crate) const INVALID_PARAMS: i32 = -32602;
pub(crate) const INTERNAL_ERROR: i32 = -32603;

/// A response to a request that indicates an error occurred.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JSONRPCError {
    pub jsonrpc: String,
    pub id: RequestId,
    pub error: ErrorObject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorObject {
    /// The error type that occurred.
    pub code: i32,
    /// A short description of the error. The message SHOULD be limited to a
    /// concise single sentence.
    pub message: String,
    /// Additional information about the error. The value of this member is
    /// defined by the sender (e.g. detailed error information, nested
    /// errors etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// Empty result
/// A response that indicates success but carries no data.
pub(crate) type EmptyResult = Result;

/// After receiving an initialize request from the client, the server sends this
/// response.
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
    ///
    /// This can be used by clients to improve the LLM's understanding of
    /// available tools, resources, etc. It can be thought of like a "hint"
    /// to the model. For example, this information MAY be added to the
    /// system prompt.
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

/// Capabilities a client may support. Known capabilities are defined here, in
/// this schema, but this is not a closed set: any client can define its own,
/// additional capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientCapabilities {
    /// Experimental, non-standard capabilities that the client supports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, Value>>,
    /// Present if the client supports listing roots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapability>,
    /// Present if the client supports sampling from an LLM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Value>,
    /// Present if the client supports elicitation requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<Value>,
}

impl ClientCapabilities {
    /// Create a new empty ClientCapabilities
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an experimental capability
    pub fn with_experimental_capability(mut self, key: impl Into<String>, value: Value) -> Self {
        self.experimental
            .get_or_insert_with(HashMap::new)
            .insert(key.into(), value);
        self
    }

    /// Enable roots capability.
    ///
    /// list_changed indicates whether the client supports notifications for changes to the roots
    /// list.
    pub fn with_roots_capability(mut self, list_changed: bool) -> Self {
        self.roots = Some(RootsCapability {
            list_changed: Some(list_changed),
        });
        self
    }

    /// Enable sampling capability
    pub fn with_sampling(mut self) -> Self {
        self.sampling = Some(Value::Object(serde_json::Map::new()));
        self
    }

    /// Enable elicitation capability
    pub fn with_elicitation(mut self) -> Self {
        self.elicitation = Some(Value::Object(serde_json::Map::new()));
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootsCapability {
    /// Whether the client supports notifications for changes to the roots list.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Capabilities that a server may support. Known capabilities are defined here,
/// in this schema, but this is not a closed set: any server can define its own,
/// additional capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerCapabilities {
    /// Experimental, non-standard capabilities that the server supports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, Value>>,
    /// Present if the server supports sending log messages to the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<Value>,
    /// Present if the server supports argument autocompletion suggestions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completions: Option<Value>,
    /// Present if the server offers any prompt templates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
    /// Present if the server offers any resources to read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    /// Present if the server offers any tools to call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
}

impl ServerCapabilities {
    /// Enable experimental capabilities
    pub fn with_experimental(mut self, experimental: HashMap<String, Value>) -> Self {
        self.experimental = Some(experimental);
        self
    }

    /// Enable logging capability
    pub fn with_logging(mut self) -> Self {
        self.logging = Some(Value::Object(serde_json::Map::new()));
        self
    }

    /// Enable completions capability
    pub fn with_completions(mut self) -> Self {
        self.completions = Some(Value::Object(serde_json::Map::new()));
        self
    }

    /// Enable prompts capability with optional list_changed support
    pub fn with_prompts(mut self, list_changed: Option<bool>) -> Self {
        self.prompts = Some(PromptsCapability { list_changed });
        self
    }

    /// Enable resources capability with optional subscribe and list_changed support
    pub fn with_resources(mut self, subscribe: Option<bool>, list_changed: Option<bool>) -> Self {
        self.resources = Some(ResourcesCapability {
            subscribe,
            list_changed,
        });
        self
    }

    /// Enable tools capability with optional list_changed support
    pub fn with_tools(mut self, list_changed: Option<bool>) -> Self {
        self.tools = Some(ToolsCapability { list_changed });
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptsCapability {
    /// Whether this server supports notifications for changes to the prompt
    /// list.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesCapability {
    /// Whether this server supports subscribing to resource updates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    /// Whether this server supports notifications for changes to the resource
    /// list.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCapability {
    /// Whether this server supports notifications for changes to the tool list.
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Describes the name and version of an MCP implementation.
// Extends BaseMetadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    /// The name of the MCP implementation.
    pub name: String,
    pub version: String,

    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl Implementation {
    /// Create a new Implementation with the given name and version
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            title: None,
        }
    }

    /// Set the title for the implementation
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// The server's response to a resources/list request from the client.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListResourcesResult {
    pub resources: Vec<Resource>,
    /// An opaque token representing the pagination position after the last
    /// returned result. If present, there may be more results available.
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

impl ListResourcesResult {
    /// Create a new empty ListResourcesResult
    pub fn new() -> Self {
        Self {
            resources: Vec::new(),
            next_cursor: None,
        }
    }

    /// Add a resource to the list
    pub fn with_resource(mut self, resource: Resource) -> Self {
        self.resources.push(resource);
        self
    }

    /// Add multiple resources to the list
    pub fn with_resources(mut self, resources: impl IntoIterator<Item = Resource>) -> Self {
        self.resources.extend(resources);
        self
    }

    /// Set the pagination cursor
    pub fn with_cursor(mut self, cursor: impl Into<Cursor>) -> Self {
        self.next_cursor = Some(cursor.into());
        self
    }
}

/// The server's response to a resources/templates/list request from the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourceTemplatesResult {
    #[serde(rename = "resourceTemplates")]
    pub resource_templates: Vec<ResourceTemplate>,
    /// An opaque token representing the pagination position after the last
    /// returned result. If present, there may be more results available.
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

/// The server's response to a resources/read request from the client.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContents>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata
    /// to their responses.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

impl ReadResourceResult {
    /// Create a new empty ReadResourceResult
    pub fn new() -> Self {
        Self {
            contents: Vec::new(),
            meta: None,
        }
    }

    /// Add content to the result
    pub fn with_content(mut self, content: ResourceContents) -> Self {
        self.contents.push(content);
        self
    }

    /// Add multiple contents to the result
    pub fn with_contents(mut self, contents: impl IntoIterator<Item = ResourceContents>) -> Self {
        self.contents.extend(contents);
        self
    }

    /// Set the metadata
    pub fn with_meta(mut self, meta: HashMap<String, Value>) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Add a single metadata entry
    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        self.meta
            .get_or_insert_with(HashMap::new)
            .insert(key.into(), value);
        self
    }
}

/// A known resource that the server is capable of reading.
// Extends BaseMetadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// The URI of this resource.
    pub uri: String,
    /// A human-readable name for this resource.
    ///
    /// This can be used by clients to populate UI elements.
    pub name: String,

    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// A description of what this resource represents.
    ///
    /// This can be used by clients to improve the LLM's understanding of
    /// available resources. It can be thought of like a "hint" to the
    /// model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The MIME type of this resource, if known.
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Optional annotations for the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// The size of the raw resource content, in bytes (i.e., before base64
    /// encoding or any tokenization), if known.
    ///
    /// This can be used by Hosts to display file sizes and estimate context
    /// window usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

/// A template description for resources available on the server.
// Extends BaseMetadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceTemplate {
    /// A URI template (according to RFC 6570) that can be used to construct
    /// resource URIs.
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    /// A human-readable name for the type of resource this template refers to.
    ///
    /// This can be used by clients to populate UI elements.
    pub name: String,

    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// A description of what this template is for.
    ///
    /// This can be used by clients to improve the LLM's understanding of
    /// available resources. It can be thought of like a "hint" to the
    /// model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The MIME type for all resources that match this template. This should
    /// only be included if all resources matching this template have the
    /// same type.
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// Optional annotations for the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

/// The contents of a specific resource or sub-resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResourceContents {
    Text(TextResourceContents),
    Blob(BlobResourceContents),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextResourceContents {
    /// The URI of this resource.
    pub uri: String,
    /// The MIME type of this resource, if known.
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// The text of the item. This must only be set if the item can actually be
    /// represented as text (not binary data).
    pub text: String,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobResourceContents {
    /// The URI of this resource.
    pub uri: String,
    /// The MIME type of this resource, if known.
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// A base64-encoded string representing the binary data of the item.
    pub blob: String,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

/// The server's response to a prompts/list request from the client.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListPromptsResult {
    pub prompts: Vec<Prompt>,
    /// An opaque token representing the pagination position after the last
    /// returned result. If present, there may be more results available.
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

impl ListPromptsResult {
    /// Create a new empty ListPromptsResult
    pub fn new() -> Self {
        Self {
            prompts: Vec::new(),
            next_cursor: None,
        }
    }

    /// Add a prompt to the list
    pub fn with_prompt(mut self, prompt: Prompt) -> Self {
        self.prompts.push(prompt);
        self
    }

    /// Add multiple prompts to the list
    pub fn with_prompts(mut self, prompts: impl IntoIterator<Item = Prompt>) -> Self {
        self.prompts.extend(prompts);
        self
    }

    /// Set the pagination cursor
    pub fn with_cursor(mut self, cursor: impl Into<Cursor>) -> Self {
        self.next_cursor = Some(cursor.into());
        self
    }
}

/// The server's response to a prompts/get request from the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPromptResult {
    /// An optional description for the prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata
    /// to their responses.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// A prompt or prompt template that the server offers.
// Extends BaseMetadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    /// The name of the prompt or prompt template.
    pub name: String,

    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// An optional description of what this prompt provides
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// A list of arguments to use for templating the prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

/// Describes an argument that a prompt can accept.
// Extends BaseMetadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    /// The name of the argument.
    pub name: String,

    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// A human-readable description of the argument.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether this argument must be provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

/// The sender or recipient of messages and data in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// Describes a message returned as part of a prompt.
///
/// This is similar to `SamplingMessage`, but also supports the embedding of
/// resources from the MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    pub role: Role,
    pub content: Content,
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

/// The contents of a resource, embedded into a prompt or tool call result.
///
/// It is up to the client how best to render embedded resources for the benefit
/// of the LLM and/or the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedResource {
    pub resource: ResourceContents,
    /// Optional annotations for the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

/// The server's response to a tools/list request from the client.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListToolsResult {
    pub tools: Vec<Tool>,
    /// An opaque token representing the pagination position after the last
    /// returned result. If present, there may be more results available.
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

impl ListToolsResult {
    /// Create a new empty ListToolsResult
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            next_cursor: None,
        }
    }

    /// Add a tool to the list
    pub fn with_tool(mut self, tool: Tool) -> Self {
        self.tools.push(tool);
        self
    }

    /// Add multiple tools to the list
    pub fn with_tools(mut self, tools: impl IntoIterator<Item = Tool>) -> Self {
        self.tools.extend(tools);
        self
    }

    /// Set the pagination cursor
    pub fn with_cursor(mut self, cursor: impl Into<Cursor>) -> Self {
        self.next_cursor = Some(cursor.into());
        self
    }
}

/// The server's response to a tool call.
///
/// Any errors that originate from the tool SHOULD be reported inside the result
/// object, with `isError` set to true, _not_ as an MCP protocol-level error
/// response. Otherwise, the LLM would not be able to see that an error occurred
/// and self-correct.
///
/// However, any errors in _finding_ the tool, an error indicating that the
/// server does not support tool calls, or any other exceptional conditions,
/// should be reported as an MCP error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    pub content: Vec<Content>,
    /// Whether the tool call ended in an error.
    ///
    /// If not set, this is assumed to be false (the call was successful).
    #[serde(rename = "isError", skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    /// Optional structured content representing the tool's output.
    #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata
    /// to their responses.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

impl CallToolResult {
    /// Create a new CallToolResult with empty content
    pub fn new() -> Self {
        Self {
            content: Vec::new(),
            is_error: None,
            structured_content: None,
            meta: None,
        }
    }

    /// Add content to the result
    pub fn with_content(mut self, content: Content) -> Self {
        self.content.push(content);
        self
    }

    /// Add text content to the result
    pub fn with_text_content(mut self, text: impl Into<String>) -> Self {
        self.content.push(Content::Text(TextContent {
            text: text.into(),
            annotations: None,
            _meta: None,
        }));
        self
    }

    /// Set the error flag
    pub fn is_error(mut self, is_error: bool) -> Self {
        self.is_error = Some(is_error);
        self
    }

    /// Set the structured content
    pub fn with_structured_content(mut self, content: Value) -> Self {
        self.structured_content = Some(content);
        self
    }

    /// Set the metadata
    pub fn with_meta(mut self, meta: HashMap<String, Value>) -> Self {
        self.meta = Some(meta);
        self
    }

    /// Add a single metadata entry
    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        self.meta
            .get_or_insert_with(HashMap::new)
            .insert(key.into(), value);
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
/// NOTE: all properties in ToolAnnotations are **hints**.
/// They are not guaranteed to provide a faithful description of
/// tool behavior (including descriptive properties like `title`).
///
/// Clients should never make tool use decisions based on ToolAnnotations
/// received from untrusted servers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolAnnotations {
    /// A human-readable title for the tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// If true, the tool does not modify its environment.
    ///
    /// Default: false
    #[serde(rename = "readOnlyHint", skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    /// If true, the tool may perform destructive updates to its environment.
    /// If false, the tool performs only additive updates.
    ///
    /// (This property is meaningful only when `readOnlyHint == false`)
    ///
    /// Default: true
    #[serde(rename = "destructiveHint", skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    /// If true, calling the tool repeatedly with the same arguments
    /// will have no additional effect on the its environment.
    ///
    /// (This property is meaningful only when `readOnlyHint == false`)
    ///
    /// Default: false
    #[serde(rename = "idempotentHint", skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    /// If true, this tool may interact with an "open world" of external
    /// entities. If false, the tool's domain of interaction is closed.
    /// For example, the world of a web search tool is open, whereas that
    /// of a memory tool is not.
    ///
    /// Default: true
    #[serde(rename = "openWorldHint", skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

/// Definition for a tool the client can call.
// Extends BaseMetadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// The name of the tool.
    pub name: String,

    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// A human-readable description of the tool.
    ///
    /// This can be used by clients to improve the LLM's understanding of
    /// available tools. It can be thought of like a "hint" to the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// A JSON Schema object defining the expected parameters for the tool.
    #[serde(rename = "inputSchema")]
    pub input_schema: ToolInputSchema,
    /// A JSON Schema object defining the expected output of the tool.
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<ToolOutputSchema>,
    /// Optional additional tool information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

impl Tool {
    /// Create a new Tool with a name and input schema
    pub fn new(name: impl Into<String>, schema: ToolInputSchema) -> Self {
        Self {
            name: name.into(),
            title: None,
            description: None,
            input_schema: schema,
            output_schema: None,
            annotations: None,
            _meta: None,
        }
    }

    /// Set the description for the tool
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the title for the tool
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the output schema for the tool
    pub fn with_output_schema(mut self, schema: ToolOutputSchema) -> Self {
        self.output_schema = Some(schema);
        self
    }

    /// Set the title annotation (different from the tool's title field)
    pub fn with_annotation_title(mut self, title: impl Into<String>) -> Self {
        self.annotations.get_or_insert_with(Default::default).title = Some(title.into());
        self
    }

    /// Set the read_only_hint annotation
    pub fn with_read_only_hint(mut self, read_only: bool) -> Self {
        self.annotations
            .get_or_insert_with(Default::default)
            .read_only_hint = Some(read_only);
        self
    }

    /// Set the destructive_hint annotation
    pub fn with_destructive_hint(mut self, destructive: bool) -> Self {
        self.annotations
            .get_or_insert_with(Default::default)
            .destructive_hint = Some(destructive);
        self
    }

    /// Set the idempotent_hint annotation
    pub fn with_idempotent_hint(mut self, idempotent: bool) -> Self {
        self.annotations
            .get_or_insert_with(Default::default)
            .idempotent_hint = Some(idempotent);
        self
    }

    /// Set the open_world_hint annotation
    pub fn with_open_world_hint(mut self, open_world: bool) -> Self {
        self.annotations
            .get_or_insert_with(Default::default)
            .open_world_hint = Some(open_world);
        self
    }

    /// Set all annotations at once
    pub fn with_annotations(mut self, annotations: ToolAnnotations) -> Self {
        self.annotations = Some(annotations);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

impl Default for ToolInputSchema {
    fn default() -> Self {
        Self {
            schema_type: "object".to_string(),
            properties: None,
            required: None,
        }
    }
}

impl ToolInputSchema {
    /// Add a property to the schema
    pub fn with_property(mut self, name: impl Into<String>, schema: Value) -> Self {
        self.properties
            .get_or_insert_with(HashMap::new)
            .insert(name.into(), schema);
        self
    }

    /// Add multiple properties at once
    pub fn with_properties(mut self, properties: HashMap<String, Value>) -> Self {
        self.properties = Some(properties);
        self
    }

    /// Mark a property as required
    pub fn with_required(mut self, name: impl Into<String>) -> Self {
        self.required.get_or_insert_with(Vec::new).push(name.into());
        self
    }

    /// Mark multiple properties as required
    pub fn with_required_properties(
        mut self,
        names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let required = self.required.get_or_insert_with(Vec::new);
        required.extend(names.into_iter().map(|n| n.into()));
        self
    }

    /// Create a ToolInputSchema from a type that implements schemars::JsonSchema
    pub fn from_json_schema<T: schemars::JsonSchema>() -> Self {
        let schema = schemars::schema_for!(T);

        // Get the underlying JSON value from the Schema
        let schema_value = schema.as_value();

        // Get the schema object if it exists
        let schema_obj = schema_value.as_object();

        // Extract type - default to "object" if not specified
        let schema_type = schema_obj
            .and_then(|obj| obj.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("object")
            .to_string();

        // Extract properties
        let properties = schema_obj
            .and_then(|obj| obj.get("properties"))
            .and_then(|v| v.as_object())
            .map(|props| {
                props
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<HashMap<_, _>>()
            });

        // Extract required fields
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
}

/// A JSON Schema object defining the expected output of a tool.
pub type ToolOutputSchema = ToolInputSchema;

/// The severity of a log message.
///
/// These map to syslog message severities, as specified in RFC-5424:
/// https://datatracker.ietf.org/doc/html/rfc5424#section-6.2.1
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoggingLevel {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageParams {
    pub messages: Vec<SamplingMessage>,
    /// The server's preferences for which model to select. The client MAY
    /// ignore these preferences.
    #[serde(rename = "modelPreferences", skip_serializing_if = "Option::is_none")]
    pub model_preferences: Option<ModelPreferences>,
    /// An optional system prompt the server wants to use for sampling. The
    /// client MAY modify or omit this prompt.
    #[serde(rename = "systemPrompt", skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// A request to include context from one or more MCP servers (including the
    /// caller), to be attached to the prompt. The client MAY ignore this
    /// request.
    #[serde(rename = "includeContext", skip_serializing_if = "Option::is_none")]
    pub include_context: Option<IncludeContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// The maximum number of tokens to sample, as requested by the server. The
    /// client MAY choose to sample fewer tokens than requested.
    #[serde(rename = "maxTokens")]
    pub max_tokens: i64,
    #[serde(rename = "stopSequences", skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Optional metadata to pass through to the LLM provider. The format of
    /// this metadata is provider-specific.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IncludeContext {
    None,
    ThisServer,
    AllServers,
}

/// The client's response to a sampling/create_message request from the server.
/// The client should inform the user before returning the sampled message, to
/// allow them to inspect the response (human in the loop) and decide whether to
/// allow the server to see it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageResult {
    pub role: Role,
    pub content: SamplingContent,
    /// The name of the model that generated the message.
    pub model: String,
    /// The reason why sampling stopped, if known.
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata
    /// to their responses.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    EndTurn,
    StopSequence,
    MaxTokens,
    #[serde(untagged)]
    Other(String),
}

/// Describes a message issued to or received from an LLM API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingMessage {
    pub role: Role,
    pub content: SamplingContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SamplingContent {
    Text(TextContent),
    Image(ImageContent),
    Audio(AudioContent),
}

/// Optional annotations for the client. The client can use annotations to
/// inform how objects are used or displayed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Annotations {
    /// Describes who the intended customer of this object or data is.
    ///
    /// It can include multiple entries to indicate content useful for multiple
    /// audiences (e.g., `["user", "assistant"]`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Vec<Role>>,
    /// Describes how important this data is for operating the server.
    ///
    /// A value of 1 means "most important," and indicates that the data is
    /// effectively required, while 0 means "least important," and indicates
    /// that the data is entirely optional.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<f64>,
    /// The last time this object was modified, in ISO 8601 format.
    #[serde(rename = "lastModified", skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
}

/// Text provided to or from an LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextContent {
    /// The text content of the message.
    pub text: String,
    /// Optional annotations for the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

/// An image provided to or from an LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContent {
    /// The base64-encoded image data.
    pub data: String,
    /// The MIME type of the image. Different providers may support different
    /// image types.
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// Optional annotations for the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

/// Audio provided to or from an LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioContent {
    /// The base64-encoded audio data.
    pub data: String,
    /// The MIME type of the audio. Different providers may support different
    /// audio types.
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// Optional annotations for the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

/// The server's preferences for model selection, requested of the client during
/// sampling.
///
/// Because LLMs can vary along multiple dimensions, choosing the "best" model
/// is rarely straightforward.  Different models excel in different areas—some
/// are faster but less capable, others are more capable but more expensive, and
/// so on. This interface allows servers to express their priorities across
/// multiple dimensions to help clients make an appropriate selection for their
/// use case.
///
/// These preferences are always advisory. The client MAY ignore them. It is
/// also up to the client to decide how to interpret these preferences and how
/// to balance them against other considerations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPreferences {
    /// Optional hints to use for model selection.
    ///
    /// If multiple hints are specified, the client MUST evaluate them in order
    /// (such that the first match is taken).
    ///
    /// The client SHOULD prioritize these hints over the numeric priorities,
    /// but MAY still use the priorities to select from ambiguous matches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<ModelHint>>,
    /// How much to prioritize cost when selecting a model. A value of 0 means
    /// cost is not important, while a value of 1 means cost is the most
    /// important factor.
    #[serde(rename = "costPriority", skip_serializing_if = "Option::is_none")]
    pub cost_priority: Option<f64>,
    /// How much to prioritize sampling speed (latency) when selecting a model.
    /// A value of 0 means speed is not important, while a value of 1 means
    /// speed is the most important factor.
    #[serde(rename = "speedPriority", skip_serializing_if = "Option::is_none")]
    pub speed_priority: Option<f64>,
    /// How much to prioritize intelligence and capabilities when selecting a
    /// model. A value of 0 means intelligence is not important, while a value
    /// of 1 means intelligence is the most important factor.
    #[serde(
        rename = "intelligencePriority",
        skip_serializing_if = "Option::is_none"
    )]
    pub intelligence_priority: Option<f64>,
}

/// Hints to use for model selection.
///
/// Keys not declared here are currently left unspecified by the spec and are up
/// to the client to interpret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelHint {
    /// A hint for a model name.
    ///
    /// The client SHOULD treat this as a substring of a model name; for
    /// example:
    ///  - `claude-3-5-sonnet` should match `claude-3-5-sonnet-20241022`
    ///  - `sonnet` should match `claude-3-5-sonnet-20241022`,
    ///    `claude-3-sonnet-20240229`, etc.
    ///  - `claude` should match any Claude model
    ///
    /// The client MAY also map the string to a different provider's model name
    /// or a different model family, as long as it fills a similar niche;
    /// for example:
    ///  - `gemini-1.5-flash` could match `claude-3-haiku-20240307`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// Autocomplete

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgumentInfo {
    /// The name of the argument
    pub name: String,
    /// The value of the argument to use for completion matching.
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Reference {
    #[serde(rename = "ref/resource")]
    Resource(ResourceReference),
    #[serde(rename = "ref/prompt")]
    Prompt(PromptReference),
}

/// A reference to a resource or resource template definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceReference {
    /// The URI or URI template of the resource.
    pub uri: String,
}

/// Identifies a prompt.
// Extends BaseMetadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptReference {
    /// The name of the prompt or prompt template.
    pub name: String,

    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// The server's response to a completion/complete request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteResult {
    pub completion: CompletionInfo,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata
    /// to their responses.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionInfo {
    /// An array of completion values. Must not exceed 100 items.
    pub values: Vec<String>,
    /// The total number of completion options available. This can exceed the
    /// number of values actually sent in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
    /// Indicates whether there are additional completion options beyond those
    /// provided in the current response, even if the exact total is
    /// unknown.
    #[serde(rename = "hasMore", skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
}

// Roots

/// The client's response to a roots/list request from the server.
/// This result contains an array of Root objects, each representing a root
/// directory or file that the server can operate on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRootsResult {
    pub roots: Vec<Root>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata
    /// to their responses.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// Represents a root directory or file that the server can operate on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Root {
    /// The URI identifying the root. This *must* start with file:// for now.
    /// This restriction may be relaxed in future versions of the protocol to
    /// allow other URI schemes.
    pub uri: String,
    /// An optional name for the root. This can be used to provide a
    /// human-readable identifier for the root, which may be useful for
    /// display purposes or for referencing the root in other parts of the
    /// application.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// meta is reserved by the protocol to allow clients and servers to attach additional metadata.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

// Messages sent from the client to the server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub(crate) enum ClientRequest {
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "initialize")]
    Initialize {
        /// The latest version of the Model Context Protocol that the client
        /// supports. The client MAY decide to support older versions as well.
        #[serde(rename = "protocolVersion")]
        protocol_version: String,
        capabilities: ClientCapabilities,
        #[serde(rename = "clientInfo")]
        client_info: Implementation,
    },
    #[serde(rename = "completion/complete")]
    Complete {
        #[serde(rename = "ref")]
        reference: Reference,
        /// The argument's information
        argument: ArgumentInfo,
        /// Additional context for the completion request
        #[serde(skip_serializing_if = "Option::is_none")]
        context: Option<CompleteContext>,
    },
    #[serde(rename = "logging/setLevel")]
    SetLevel {
        /// The level of logging that the client wants to receive from the
        /// server.
        level: LoggingLevel,
    },
    #[serde(rename = "prompts/get")]
    GetPrompt {
        /// The name of the prompt or prompt template.
        name: String,
        /// Arguments to use for templating the prompt.
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<HashMap<String, String>>,
    },
    #[serde(rename = "prompts/list")]
    ListPrompts {
        /// An opaque token representing the current pagination position.
        /// If provided, the server should return results starting after this cursor.
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<Cursor>,
    },
    #[serde(rename = "resources/list")]
    ListResources {
        /// An opaque token representing the current pagination position.
        /// If provided, the server should return results starting after this cursor.
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<Cursor>,
    },
    #[serde(rename = "resources/templates/list")]
    ListResourceTemplates {
        /// An opaque token representing the current pagination position.
        /// If provided, the server should return results starting after this cursor.
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<Cursor>,
    },
    #[serde(rename = "resources/read")]
    ReadResource {
        /// The URI of the resource to read.
        uri: String,
    },
    #[serde(rename = "resources/subscribe")]
    Subscribe {
        /// The URI of the resource to subscribe to.
        uri: String,
    },
    #[serde(rename = "resources/unsubscribe")]
    Unsubscribe {
        /// The URI of the resource to unsubscribe from.
        uri: String,
    },
    #[serde(rename = "tools/call")]
    CallTool {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        arguments: Option<HashMap<String, Value>>,
    },
    #[serde(rename = "tools/list")]
    ListTools {
        /// An opaque token representing the current pagination position.
        /// If provided, the server should return results starting after this cursor.
        #[serde(skip_serializing_if = "Option::is_none")]
        cursor: Option<Cursor>,
    },
}

impl ClientRequest {
    /// Get the method name for this request
    pub fn method(&self) -> &'static str {
        match self {
            ClientRequest::Ping => "ping",
            ClientRequest::Initialize { .. } => "initialize",
            ClientRequest::Complete { .. } => "completion/complete",
            ClientRequest::SetLevel { .. } => "logging/setLevel",
            ClientRequest::GetPrompt { .. } => "prompts/get",
            ClientRequest::ListPrompts { .. } => "prompts/list",
            ClientRequest::ListResources { .. } => "resources/list",
            ClientRequest::ListResourceTemplates { .. } => "resources/templates/list",
            ClientRequest::ReadResource { .. } => "resources/read",
            ClientRequest::Subscribe { .. } => "resources/subscribe",
            ClientRequest::Unsubscribe { .. } => "resources/unsubscribe",
            ClientRequest::CallTool { .. } => "tools/call",
            ClientRequest::ListTools { .. } => "tools/list",
        }
    }
}

impl RequestMethod for ClientRequest {
    fn method(&self) -> &'static str {
        self.method()
    }
}

/// Notifications sent from the client to the server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum ClientNotification {
    // Cancellation
    /// This notification can be sent by either side to indicate that it is
    /// cancelling a previously-issued request.
    ///
    /// The request SHOULD still be in-flight, but due to communication latency, it
    /// is always possible that this notification MAY arrive after the request has
    /// already finished.
    ///
    /// This notification indicates that the result will be unused, so any
    /// associated processing SHOULD cease.
    ///
    /// A client MUST NOT attempt to cancel its `initialize` request.
    #[serde(rename = "notifications/cancelled")]
    Cancelled {
        /// The ID of the request to cancel.
        #[serde(rename = "requestId")]
        request_id: RequestId,
        /// An optional string describing the reason for the cancellation.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename = "notifications/progress")]
    Progress {
        /// The progress token which was given in the initial request.
        #[serde(rename = "progressToken")]
        progress_token: ProgressToken,
        /// The progress thus far.
        progress: f64,
        /// Total number of items to process, if known.
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<f64>,
        /// An optional message describing the current progress.
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },

    /// This notification is sent from the client to the server after initialization
    /// has finished.
    #[serde(rename = "notifications/initialized")]
    Initialized,

    /// A notification from the client to the server, informing it that the list of
    /// roots has changed. This notification should be sent whenever the client
    /// adds, removes, or modifies any root. The server should then request an
    /// updated list of roots using the ListRootsRequest.
    #[serde(rename = "notifications/roots/list_changed")]
    RootsListChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ClientResult {
    Empty(EmptyResult),
    CreateMessage(CreateMessageResult),
    ListRoots(ListRootsResult),
    Elicit(ElicitResult),
}

/// Requests sent from the server to the client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum ServerRequest {
    #[serde(rename = "ping")]
    Ping,

    /// A request from the server to sample an LLM via the client. The client has
    /// full discretion over which model to select. The client should also inform
    /// the user before beginning sampling, to allow them to inspect the request
    /// (human in the loop) and decide whether to approve it.
    #[serde(rename = "sampling/createMessage")]
    CreateMessage(Box<CreateMessageParams>),

    #[serde(rename = "roots/list")]
    ListRoots,

    /// A request from the server to elicit additional information from the client.
    /// This allows servers to ask for user input during execution.
    #[serde(rename = "elicitation/create")]
    Elicit(ElicitParams),
}

impl ServerRequest {
    /// Get the method name for this request
    pub fn method(&self) -> &'static str {
        match self {
            ServerRequest::Ping => "ping",
            ServerRequest::CreateMessage { .. } => "sampling/createMessage",
            ServerRequest::ListRoots => "roots/list",
            ServerRequest::Elicit { .. } => "elicitation/create",
        }
    }
}

impl RequestMethod for ServerRequest {
    fn method(&self) -> &'static str {
        self.method()
    }
}

/// Notifications sent from the server to the client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub enum ServerNotification {
    /// This notification can be sent by either side to indicate that it is
    /// cancelling a previously-issued request.
    ///
    /// The request SHOULD still be in-flight, but due to communication latency, it
    /// is always possible that this notification MAY arrive after the request has
    /// already finished.
    ///
    /// This notification indicates that the result will be unused, so any
    /// associated processing SHOULD cease.
    ///
    /// A client MUST NOT attempt to cancel its `initialize` request.
    #[serde(rename = "notifications/cancelled")]
    Cancelled {
        /// The ID of the request to cancel.
        #[serde(rename = "requestId")]
        request_id: RequestId,
        /// An optional string describing the reason for the cancellation.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename = "notifications/progress")]
    Progress {
        /// The progress token which was given in the initial request.
        #[serde(rename = "progressToken")]
        progress_token: ProgressToken,
        /// The progress thus far.
        progress: f64,
        /// Total number of items to process, if known.
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<f64>,
        /// An optional message describing the current progress.
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Notification of a log message passed from server to client. If no
    /// logging/setLevel request has been sent from the client, the server MAY
    /// decide which messages to send automatically.
    #[serde(rename = "notifications/message")]
    LoggingMessage {
        /// The severity of this log message.
        level: LoggingLevel,
        /// An optional name of the logger issuing this message.
        #[serde(skip_serializing_if = "Option::is_none")]
        logger: Option<String>,
        /// The data to be logged.
        data: Value,
    },

    /// A notification from the server to the client, informing it that a resource
    /// has changed and may need to be read again. This should only be sent if the
    /// client previously sent a resources/subscribe request.
    #[serde(rename = "notifications/resources/updated")]
    ResourceUpdated {
        /// The URI of the resource that has been updated.
        uri: String,
    },

    /// An optional notification from the server to the client, informing it that
    /// the list of resources it can read from has changed. This may be issued by
    /// servers without any previous subscription from the client.
    #[serde(rename = "notifications/resources/list_changed")]
    ResourceListChanged,

    /// An optional notification from the server to the client, informing it that
    /// the list of tools it offers has changed. This may be issued by servers
    /// without any previous subscription from the client.
    #[serde(rename = "notifications/tools/list_changed")]
    ToolListChanged,

    /// An optional notification from the server to the client, informing it that
    /// the list of prompts it offers has changed. This may be issued by servers
    /// without any previous subscription from the client.
    #[serde(rename = "notifications/prompts/list_changed")]
    PromptListChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ServerResult {
    Empty(EmptyResult),
    Initialize(InitializeResult),
    Complete(CompleteResult),
    GetPrompt(GetPromptResult),
    ListPrompts(ListPromptsResult),
    ListResourceTemplates(ListResourceTemplatesResult),
    ListResources(ListResourcesResult),
    ReadResource(ReadResourceResult),
    CallTool(CallToolResult),
    ListTools(ListToolsResult),
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemars::JsonSchema;

    #[derive(JsonSchema, Serialize)]
    struct TestInput {
        name: String,
        age: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        email: Option<String>,
    }

    #[test]
    fn test_tool_input_schema_from_json_schema() {
        let schema = ToolInputSchema::from_json_schema::<TestInput>();

        assert_eq!(schema.schema_type, "object");

        let properties = schema.properties.expect("Should have properties");
        assert!(properties.contains_key("name"));
        assert!(properties.contains_key("age"));
        assert!(properties.contains_key("email"));

        let required = schema.required.expect("Should have required fields");
        assert!(required.contains(&"name".to_string()));
        assert!(required.contains(&"age".to_string()));
        assert!(!required.contains(&"email".to_string()));
    }

    #[derive(JsonSchema, Serialize)]
    struct ComplexInput {
        id: i64,
        tags: Vec<String>,
        metadata: HashMap<String, String>,
    }

    #[test]
    fn test_complex_schema_conversion() {
        let schema = ToolInputSchema::from_json_schema::<ComplexInput>();

        assert_eq!(schema.schema_type, "object");

        let properties = schema.properties.expect("Should have properties");
        assert!(properties.contains_key("id"));
        assert!(properties.contains_key("tags"));
        assert!(properties.contains_key("metadata"));

        // Verify array type for tags
        let tags_schema = &properties["tags"];
        assert_eq!(
            tags_schema.get("type").and_then(|v| v.as_str()),
            Some("array")
        );

        // Verify object type for metadata
        let metadata_schema = &properties["metadata"];
        assert_eq!(
            metadata_schema.get("type").and_then(|v| v.as_str()),
            Some("object")
        );
    }

    #[test]
    fn test_paginated_request_serialization() {
        // Test ListTools with cursor
        let request = ClientRequest::ListTools {
            cursor: Some("test-cursor".into()),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["method"], "tools/list");
        assert_eq!(json["cursor"], "test-cursor");

        // Test ListTools without cursor
        let request = ClientRequest::ListTools { cursor: None };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["method"], "tools/list");
        assert!(!json.as_object().unwrap().contains_key("cursor"));

        // Test ListResources with cursor
        let request = ClientRequest::ListResources {
            cursor: Some("res-cursor".into()),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["method"], "resources/list");
        assert_eq!(json["cursor"], "res-cursor");

        // Test ListPrompts with cursor
        let request = ClientRequest::ListPrompts {
            cursor: Some("prompt-cursor".into()),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["method"], "prompts/list");
        assert_eq!(json["cursor"], "prompt-cursor");

        // Test ListResourceTemplates with cursor
        let request = ClientRequest::ListResourceTemplates {
            cursor: Some("template-cursor".into()),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["method"], "resources/templates/list");
        assert_eq!(json["cursor"], "template-cursor");
    }

    #[test]
    fn test_client_capabilities_elicitation() {
        let caps = ClientCapabilities::new().with_elicitation();
        let json = serde_json::to_value(&caps).unwrap();
        assert!(json["elicitation"].is_object());
    }

    #[test]
    fn test_tool_output_schema() {
        let tool = Tool::new("test_tool", ToolInputSchema::default())
            .with_output_schema(ToolOutputSchema::default());
        let json = serde_json::to_value(&tool).unwrap();
        assert!(json["outputSchema"].is_object());
    }

    #[test]
    fn test_call_tool_result_structured_content() {
        let structured = serde_json::json!({"type": "table"});
        let result = CallToolResult::new().with_structured_content(structured.clone());
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["structuredContent"], structured);
    }

    #[test]
    fn test_annotations_last_modified() {
        let annotations = Annotations {
            audience: None,
            priority: None,
            last_modified: Some("2024-01-15T10:30:00Z".to_string()),
        };
        let json = serde_json::to_value(&annotations).unwrap();
        assert_eq!(json["lastModified"], "2024-01-15T10:30:00Z");
    }

    #[test]
    fn test_complete_request_context() {
        let request = ClientRequest::Complete {
            reference: Reference::Resource(ResourceReference {
                uri: "test://resource".to_string(),
            }),
            argument: ArgumentInfo {
                name: "arg".to_string(),
                value: "value".to_string(),
            },
            context: Some(CompleteContext::new().add_argument("sessionId", "123")),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["context"]["arguments"]["sessionId"], "123");
    }
}

// ============================================================================
// NEW PROTOCOL STRUCTS FROM 2025-06-18 SCHEMA
// ============================================================================

/// A resource that the server is capable of reading, included in a prompt or tool call result.
///
/// Note: resource links returned by tools are not guaranteed to appear in the results of `resources/list` requests.
// Extends Resource (which extends BaseMetadata)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLink {
    #[serde(rename = "type")]
    pub link_type: String, // Should be "resource_link"

    /// The URI of this resource.
    pub uri: String,

    /// A human-readable name for this resource.
    pub name: String,

    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// A description of what this resource represents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The MIME type of this resource, if known.
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    /// Optional annotations for the client.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Annotations>,

    /// The size of the raw resource content, in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,

    /// See [specification/2025-06-18/basic/index#general-fields] for notes on _meta usage.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

/// This request is sent from the client to the server when it first connects, asking it to begin initialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeRequest {
    pub method: String, // "initialize"
    pub params: InitializeParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    /// The latest version of the Model Context Protocol that the client supports.
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    #[serde(rename = "clientInfo")]
    pub client_info: Implementation,
}

/// This notification is sent from the client to the server after initialization has finished.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializedNotification {
    pub method: String, // "notifications/initialized"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, Value>>,
}

/// A ping, issued by either the server or the client, to check that the other party is still alive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingRequest {
    pub method: String, // "ping"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, Value>>,
}

/// This notification can be sent by either side to indicate that it is cancelling a previously-issued request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelledNotification {
    pub method: String, // "notifications/cancelled"
    pub params: CancelledParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelledParams {
    /// The ID of the request to cancel.
    #[serde(rename = "requestId")]
    pub request_id: RequestId,

    /// An optional string describing the reason for the cancellation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// An out-of-band notification used to inform the receiver of a progress update for a long-running request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressNotification {
    pub method: String, // "notifications/progress"
    pub params: ProgressParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressParams {
    /// The progress token which was given in the initial request.
    #[serde(rename = "progressToken")]
    pub progress_token: ProgressToken,

    /// The progress thus far. This should increase every time progress is made.
    pub progress: f64,

    /// Total number of items to process (or total progress required), if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,

    /// An optional message describing the current progress.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Base interface for paginated requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedRequest {
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<PaginatedParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedParams {
    /// An opaque token representing the current pagination position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
}

/// Base interface for paginated results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedResult {
    /// An opaque token representing the pagination position after the last returned result.
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

// Resource-related request types

/// Sent from the client to request a list of resources the server has.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourcesRequest {
    pub method: String, // "resources/list"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<PaginatedParams>,
}

/// Sent from the client to request a list of resource templates the server has.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResourceTemplatesRequest {
    pub method: String, // "resources/templates/list"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<PaginatedParams>,
}

/// Sent from the client to the server, to read a specific resource URI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResourceRequest {
    pub method: String, // "resources/read"
    pub params: ReadResourceParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadResourceParams {
    /// The URI of the resource to read.
    pub uri: String,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
}

/// Sent from the client to request resources/updated notifications from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRequest {
    pub method: String, // "resources/subscribe"
    pub params: SubscribeParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeParams {
    /// The URI of the resource to subscribe to.
    pub uri: String,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
}

/// Sent from the client to request cancellation of resources/updated notifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeRequest {
    pub method: String, // "resources/unsubscribe"
    pub params: UnsubscribeParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsubscribeParams {
    /// The URI of the resource to unsubscribe from.
    pub uri: String,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
}

/// An optional notification from the server to the client about resource list changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceListChangedNotification {
    pub method: String, // "notifications/resources/list_changed"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, Value>>,
}

/// A notification from the server to the client about a resource update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUpdatedNotification {
    pub method: String, // "notifications/resources/updated"
    pub params: ResourceUpdatedParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUpdatedParams {
    /// The URI of the resource that has been updated.
    pub uri: String,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

// Prompt-related request types

/// Sent from the client to request a list of prompts and prompt templates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPromptsRequest {
    pub method: String, // "prompts/list"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<PaginatedParams>,
}

/// Used by the client to get a prompt provided by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPromptRequest {
    pub method: String, // "prompts/get"
    pub params: GetPromptParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPromptParams {
    /// The name of the prompt or prompt template.
    pub name: String,

    /// Arguments to use for templating the prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, String>>,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
}

/// An optional notification from the server about prompt list changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptListChangedNotification {
    pub method: String, // "notifications/prompts/list_changed"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, Value>>,
}

// Tool-related request types

/// Sent from the client to request a list of tools the server has.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolsRequest {
    pub method: String, // "tools/list"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<PaginatedParams>,
}

/// Used by the client to invoke a tool provided by the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolRequest {
    pub method: String, // "tools/call"
    pub params: CallToolParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolParams {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, Value>>,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
}

/// An optional notification from the server about tool list changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolListChangedNotification {
    pub method: String, // "notifications/tools/list_changed"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, Value>>,
}

// Logging-related types

/// A request from the client to the server, to enable or adjust logging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLevelRequest {
    pub method: String, // "logging/setLevel"
    pub params: SetLevelParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLevelParams {
    /// The level of logging that the client wants to receive from the server.
    pub level: LoggingLevel,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
}

/// Notification of a log message passed from server to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingMessageNotification {
    pub method: String, // "notifications/message"
    pub params: LoggingMessageParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingMessageParams {
    /// The severity of this log message.
    pub level: LoggingLevel,

    /// An optional name of the logger issuing this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logger: Option<String>,

    /// The data to be logged, such as a string message or an object.
    pub data: Value,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

// Sampling-related types

/// A request from the server to sample an LLM via the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageRequest {
    pub method: String, // "sampling/createMessage"
    pub params: CreateMessageParams,
}

// Completion-related types

/// A request from the client to the server, to ask for completion options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteRequest {
    pub method: String, // "completion/complete"
    pub params: CompleteParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteParams {
    #[serde(rename = "ref")]
    pub reference: Reference,

    /// The argument's information
    pub argument: ArgumentInfo,

    /// Additional, optional context for completions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<CompleteContext>,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteContext {
    /// Previously-resolved variables in a URI template or prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<HashMap<String, String>>,
}

impl CompleteContext {
    /// Create a new CompleteContext with no arguments
    pub fn new() -> Self {
        Self { arguments: None }
    }

    /// Create a CompleteContext with the provided arguments
    pub fn with_arguments(arguments: HashMap<String, String>) -> Self {
        Self {
            arguments: Some(arguments),
        }
    }

    /// Add a single argument to the context
    pub fn add_argument(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let arguments = self.arguments.get_or_insert_with(HashMap::new);
        arguments.insert(key.into(), value.into());
        self
    }

    /// Set the arguments, replacing any existing ones
    pub fn set_arguments(mut self, arguments: HashMap<String, String>) -> Self {
        self.arguments = Some(arguments);
        self
    }
}

impl Default for CompleteContext {
    fn default() -> Self {
        Self::new()
    }
}

// Roots-related types

/// Sent from the server to request a list of root URIs from the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRootsRequest {
    pub method: String, // "roots/list"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, Value>>,
}

/// A notification from the client to the server about roots list changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootsListChangedNotification {
    pub method: String, // "notifications/roots/list_changed"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<HashMap<String, Value>>,
}

// Elicitation-related types

/// A request from the server to elicit additional information from the user via the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitRequest {
    pub method: String, // "elicitation/create"
    pub params: ElicitParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitParams {
    /// The message to present to the user.
    pub message: String,

    /// A restricted subset of JSON Schema.
    #[serde(rename = "requestedSchema")]
    pub requested_schema: ElicitSchema,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<RequestMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitSchema {
    #[serde(rename = "type")]
    pub schema_type: String, // "object"

    pub properties: HashMap<String, PrimitiveSchemaDefinition>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

/// The client's response to an elicitation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitResult {
    /// The user action in response to the elicitation.
    pub action: ElicitAction,

    /// The submitted form data, only present when action is "accept".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<HashMap<String, ElicitValue>>,

    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ElicitAction {
    Accept,
    Decline,
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ElicitValue {
    String(String),
    Number(f64),
    Boolean(bool),
}

/// Restricted schema definitions that only allow primitive types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum PrimitiveSchemaDefinition {
    #[serde(rename = "string")]
    String(StringSchema),
    #[serde(rename = "number")]
    Number(NumberSchema),
    #[serde(rename = "integer")]
    Integer(NumberSchema),
    #[serde(rename = "boolean")]
    Boolean(BooleanSchema),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StringSchema {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(rename = "minLength", skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u32>,

    #[serde(rename = "maxLength", skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<StringFormat>,

    // For enum support
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,

    #[serde(rename = "enumNames", skip_serializing_if = "Option::is_none")]
    pub enum_names: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StringFormat {
    Email,
    Uri,
    Date,
    DateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NumberSchema {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BooleanSchema {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<bool>,
}
