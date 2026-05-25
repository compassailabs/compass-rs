use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String },
    /// Assistant reasoning emitted by extended-thinking models (Anthropic
    /// extended thinking, DeepSeek v4-flash thinking mode). **Must** be
    /// echoed back on every subsequent turn — DeepSeek rejects the request
    /// with 400 ("reasoning_content … must be passed back to the API") and
    /// Anthropic invalidates the cache without it. The `thinking` field
    /// name matches Anthropic's wire format; `signature` is Anthropic-only
    /// (DeepSeek doesn't need it).
    Thinking {
        thinking: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Clone, Debug)]
pub struct CompletionRequest {
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolSchema>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    /// Anthropic extended-thinking budget. `Some(N)` enables thinking with
    /// `N` reserved tokens (must be ≥ 1024, and `max_tokens` must exceed it).
    /// Non-Anthropic providers ignore this field.
    pub thinking_budget_tokens: Option<u32>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CompletionResponse {
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<String>,
}
