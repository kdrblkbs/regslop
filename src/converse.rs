//! Wire types for the AWS Bedrock Converse API.
//!
//! Type, variant and field names mirror the API's JSON shapes, see
//! <https://docs.aws.amazon.com/bedrock/latest/APIReference/API_runtime_Converse.html>.
//! Unions are modelled as externally tagged enums. Response-side enums have an
//! `Unknown` fallback that keeps the raw JSON, so it can be logged and sent
//! back as-is.

#![expect(
    dead_code,
    reason = "models the API surface, not everything is used yet"
)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Request {
    pub(crate) messages: Vec<Message>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) system: Vec<SystemBlock>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) inference_config: Option<InferenceConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_config: Option<ToolConfig>,

    /// Model-specific settings passed through as-is, e.g. Claude's
    /// `{"thinking": {"type": "enabled", "budget_tokens": 4000}}`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) additional_model_request_fields: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InferenceConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) top_p: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) stop_sequences: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SystemBlock {
    Text(String),
    CachePoint(CachePoint),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolConfig {
    pub(crate) tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_choice: Option<ToolChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Tool {
    ToolSpec(ToolSpec),
    CachePoint(CachePoint),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolSpec {
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) input_schema: InputSchema,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum InputSchema {
    /// JSON Schema as a JSON object.
    Json(Value),
}

/// `{"auto": {}}`, `{"any": {}}` or `{"tool": {"name": "..."}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ToolChoice {
    Auto {},
    Any {},
    Tool { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Message {
    pub(crate) role: Role,
    pub(crate) content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ContentBlock {
    Text(String),
    ToolUse(ToolUse),
    ToolResult(ToolResult),
    ReasoningContent(Reasoning),
    CachePoint(CachePoint),
    #[serde(untagged)]
    Unknown(Value),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolUse {
    pub(crate) tool_use_id: String,
    pub(crate) name: String,
    pub(crate) input: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolResult {
    pub(crate) tool_use_id: String,
    pub(crate) content: Vec<ToolResultContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<ToolStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ToolResultContent {
    Text(String),
    Json(Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ToolStatus {
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Reasoning {
    ReasoningText(ReasoningText),
    /// Encrypted reasoning, base64. Opaque; just send it back.
    RedactedContent(String),
    #[serde(untagged)]
    Unknown(Value),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReasoningText {
    pub(crate) text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) signature: Option<String>,
}

/// Prompt-cache marker: everything before it gets cached. Serializes as
/// `{"type": "default"}`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CachePoint {
    #[serde(rename = "type")]
    pub(crate) kind: CachePointKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CachePointKind {
    #[default]
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Response {
    pub(crate) output: Output,
    pub(crate) stop_reason: StopReason,
    pub(crate) usage: Usage,
    pub(crate) metrics: Metrics,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Output {
    Message(Message),
    #[serde(untagged)]
    Unknown(Value),
}

impl Output {
    pub(crate) fn into_message(self) -> anyhow::Result<Message> {
        match self {
            Output::Message(message) => Ok(message),
            Output::Unknown(u) => anyhow::bail!("output is unknown: {u}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    GuardrailIntervened,
    ContentFiltered,
    MalformedModelOutput,
    MalformedToolUse,
    ModelContextWindowExceeded,
    /// Any value AWS adds later.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Usage {
    pub(crate) input_tokens: u32,
    pub(crate) output_tokens: u32,
    pub(crate) total_tokens: u32,
    /// Only present when prompt caching was involved.
    #[serde(default)]
    pub(crate) cache_read_input_tokens: u32,
    #[serde(default)]
    pub(crate) cache_write_input_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Metrics {
    pub(crate) latency_ms: u64,
}
