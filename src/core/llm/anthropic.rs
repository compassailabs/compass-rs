use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use serde_json::{Value, json};

use super::provider::LlmProvider;
use super::stream::{StreamFormat, TaggedStream};
use super::types::{CompletionRequest, CompletionResponse, ContentBlock, Role};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    http: Client,
    model: String,
    api_key: String,
}

impl AnthropicProvider {
    pub fn new(model: &str, api_key: &str) -> Self {
        Self {
            http: Client::new(),
            model: model.to_string(),
            api_key: api_key.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse> {
        if self.api_key.is_empty() {
            return Err(anyhow!("ANTHROPIC_API_KEY not configured"));
        }

        let body = AnthropicRequest {
            model: &self.model,
            max_tokens: req.max_tokens,
            system: &req.system,
            messages: req
                .messages
                .iter()
                .map(|m| AnthropicMessage {
                    role: match m.role {
                        Role::User => "user",
                        Role::Assistant => "assistant",
                    },
                    content: m.content.clone(),
                })
                .collect(),
            tools: req.tools.iter().map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                })
            }).collect(),
            temperature: req.temperature,
            stream: None,
        };

        let resp = self
            .http
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "anthropic {} → {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        Ok(resp.json::<CompletionResponse>().await?)
    }

    async fn completion_stream(&self, req: &CompletionRequest) -> Result<TaggedStream> {
        if self.api_key.is_empty() {
            return Err(anyhow!("ANTHROPIC_API_KEY not configured"));
        }

        let body = AnthropicRequest {
            model: &self.model,
            max_tokens: req.max_tokens,
            system: &req.system,
            messages: req
                .messages
                .iter()
                .map(|m| AnthropicMessage {
                    role: match m.role {
                        Role::User => "user",
                        Role::Assistant => "assistant",
                    },
                    content: m.content.clone(),
                })
                .collect(),
            tools: req
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.input_schema,
                    })
                })
                .collect(),
            temperature: req.temperature,
            stream: Some(true),
        };

        let resp = self
            .http
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "anthropic {} → {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }

        Ok(TaggedStream {
            stream: Box::pin(resp.bytes_stream()),
            format: StreamFormat::AnthropicSse,
            model: self.model.clone(),
            provider_name: "anthropic".to_string(),
        })
    }
}

#[derive(Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<AnthropicMessage>,
    tools: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: &'static str,
    content: Vec<ContentBlock>,
}
