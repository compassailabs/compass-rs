use anyhow::Result;
use async_trait::async_trait;

use super::stream::TaggedStream;
use super::types::{CompletionRequest, CompletionResponse};

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    async fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse>;
    async fn completion_stream(&self, req: &CompletionRequest) -> Result<TaggedStream>;
}
