use std::pin::Pin;

use bytes::Bytes;
use futures::Stream;
use serde_json::Value;

#[derive(Clone, Copy, Debug)]
pub enum StreamFormat {
    OpenAiSse,
    AnthropicSse,
}

pub struct TaggedStream {
    pub stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    pub format: StreamFormat,
    pub model: String,
    pub provider_name: String,
}

#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}
