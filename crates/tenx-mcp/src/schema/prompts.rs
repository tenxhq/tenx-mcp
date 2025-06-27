use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListPromptsResult {
    pub prompts: Vec<Prompt>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<Cursor>,
}

impl ListPromptsResult {
    pub fn new() -> Self {
        Self {
            prompts: Vec::new(),
            next_cursor: None,
        }
    }

    pub fn with_prompt(mut self, prompt: Prompt) -> Self {
        self.prompts.push(prompt);
        self
    }

    pub fn with_prompts(mut self, prompts: impl IntoIterator<Item = Prompt>) -> Self {
        self.prompts.extend(prompts);
        self
    }

    pub fn with_cursor(mut self, cursor: impl Into<Cursor>) -> Self {
        self.next_cursor = Some(cursor.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPromptResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<HashMap<String, Value>>,
}

/// A prompt or prompt template that the server offers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,

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

/// Describes an argument that a prompt can accept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,

    /// Intended for programmatic or logical use, but used as a display name in past specs or fallback (if title isn't present).
    pub name: String,
    /// Intended for UI and end-user contexts — optimized to be human-readable and easily understood,
    /// even by those unfamiliar with domain-specific terminology.
    ///
    /// If not provided, the name should be used for display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    pub role: Role,
    pub content: Content,
}
