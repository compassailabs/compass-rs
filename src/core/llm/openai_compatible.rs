use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::provider::LlmProvider;
use super::stream::{StreamFormat, TaggedStream};
use super::types::{ChatMessage, CompletionRequest, CompletionResponse, ContentBlock, Role};

pub struct OpenAiCompatibleProvider {
    http: Client,
    base_url: String,
    model: String,
    api_key: String,
    name: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(name: &str, base_url: &str, model: &str, api_key: &str) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: api_key.to_string(),
            name: name.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse> {
        if self.api_key.is_empty() {
            return Err(anyhow!("{} api key not configured", self.name));
        }

        let messages = anthropic_to_openai_messages(&req.system, &req.messages);
        let tools = anthropic_to_openai_tools(req);

        let body = OpenAiRequest {
            model: &self.model,
            messages,
            max_tokens: Some(req.max_tokens),
            tools,
            temperature: req.temperature,
            stream: false,
        };

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "{} {} → {}",
                self.name,
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }

        let parsed: OpenAiResponse = resp.json().await?;
        let Some(choice) = parsed.choices.into_iter().next() else {
            return Err(anyhow!("{} returned no choices", self.name));
        };
        Ok(openai_choice_to_completion(choice))
    }

    async fn completion_stream(&self, req: &CompletionRequest) -> Result<TaggedStream> {
        if self.api_key.is_empty() {
            return Err(anyhow!("{} api key not configured", self.name));
        }

        let messages = anthropic_to_openai_messages(&req.system, &req.messages);
        let tools = anthropic_to_openai_tools(req);

        let body = OpenAiRequest {
            model: &self.model,
            messages,
            max_tokens: Some(req.max_tokens),
            tools,
            temperature: req.temperature,
            stream: true,
        };

        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "{} {} → {}",
                self.name,
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }

        Ok(TaggedStream {
            stream: Box::pin(resp.bytes_stream()),
            format: StreamFormat::OpenAiSse,
            model: self.model.clone(),
            provider_name: self.name.clone(),
        })
    }
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
}

#[derive(Serialize)]
struct OpenAiMessage {
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize)]
struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: &'static str,
    function: OpenAiFunction,
}

#[derive(Serialize)]
struct OpenAiFunction {
    name: String,
    arguments: String,
}

fn anthropic_to_openai_messages(system: &str, msgs: &[ChatMessage]) -> Vec<OpenAiMessage> {
    let mut out: Vec<OpenAiMessage> = Vec::with_capacity(msgs.len() + 1);

    if !system.is_empty() {
        out.push(OpenAiMessage {
            role: "system",
            content: Some(system.to_string()),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for m in msgs {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };

        let mut text = String::new();
        let mut tool_calls: Vec<OpenAiToolCall> = Vec::new();
        let mut tool_results: Vec<(String, String)> = Vec::new();

        for block in &m.content {
            match block {
                ContentBlock::Text { text: t } => {
                    if !text.is_empty() {
                        text.push_str("\n\n");
                    }
                    text.push_str(t);
                }
                ContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(OpenAiToolCall {
                        id: id.clone(),
                        call_type: "function",
                        function: OpenAiFunction {
                            name: name.clone(),
                            arguments: input.to_string(),
                        },
                    });
                }
                ContentBlock::ToolResult { tool_use_id, content } => {
                    tool_results.push((tool_use_id.clone(), content.clone()));
                }
            }
        }

        if role == "user" && !tool_results.is_empty() {
            for (id, content) in &tool_results {
                out.push(OpenAiMessage {
                    role: "tool",
                    content: Some(content.clone()),
                    tool_calls: None,
                    tool_call_id: Some(id.clone()),
                });
            }
            if !text.is_empty() {
                out.push(OpenAiMessage {
                    role: "user",
                    content: Some(text),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            continue;
        }

        out.push(OpenAiMessage {
            role,
            content: if text.is_empty() { None } else { Some(text) },
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id: None,
        });
    }

    out
}

fn anthropic_to_openai_tools(req: &CompletionRequest) -> Option<Vec<Value>> {
    if req.tools.is_empty() {
        return None;
    }
    Some(
        req.tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    }
                })
            })
            .collect(),
    )
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiResponseToolCall>>,
}

#[derive(Deserialize)]
struct OpenAiResponseToolCall {
    id: String,
    function: OpenAiResponseFunction,
}

#[derive(Deserialize)]
struct OpenAiResponseFunction {
    name: String,
    arguments: String,
}

fn openai_choice_to_completion(choice: OpenAiChoice) -> CompletionResponse {
    let mut content: Vec<ContentBlock> = Vec::new();

    if let Some(text) = choice.message.content.filter(|s| !s.is_empty()) {
        content.push(ContentBlock::Text { text });
    }
    if let Some(calls) = choice.message.tool_calls {
        for c in calls {
            let input: Value = serde_json::from_str(&c.function.arguments)
                .unwrap_or(Value::Null);
            content.push(ContentBlock::ToolUse {
                id: c.id,
                name: c.function.name,
                input,
            });
        }
    }

    let stop_reason = match choice.finish_reason.as_deref() {
        Some("tool_calls") => Some("tool_use".to_string()),
        Some("stop") => Some("end_turn".to_string()),
        Some("length") => Some("max_tokens".to_string()),
        Some(other) => Some(other.to_string()),
        None => None,
    };

    CompletionResponse { content, stop_reason }
}
