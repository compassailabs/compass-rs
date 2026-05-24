use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;

use super::provider::LlmProvider;
use super::stream::TaggedStream;
use super::types::{CompletionRequest, CompletionResponse};

pub struct ProviderChain {
    providers: Vec<Arc<dyn LlmProvider>>,
}

impl ProviderChain {
    pub fn new(providers: Vec<Arc<dyn LlmProvider>>) -> Self {
        assert!(
            !providers.is_empty(),
            "ProviderChain requires at least one provider"
        );
        Self { providers }
    }
}

#[async_trait]
impl LlmProvider for ProviderChain {
    fn name(&self) -> &str {
        "chain"
    }

    fn model(&self) -> &str {
        self.providers[0].model()
    }

    async fn complete(&self, req: &CompletionRequest) -> Result<CompletionResponse> {
        let mut last_err: Option<anyhow::Error> = None;

        for (i, provider) in self.providers.iter().enumerate() {
            match provider.complete(req).await {
                Ok(resp) => {
                    if i > 0 {
                        tracing::warn!(
                            provider = provider.name(),
                            model = provider.model(),
                            "[CHAIN] fallback succeeded after {} primary failure(s)",
                            i,
                        );
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    tracing::warn!(
                        provider = provider.name(),
                        model = provider.model(),
                        error = %e,
                        "[CHAIN] provider failed, trying next",
                    );
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow!("all providers in chain failed")))
    }

    async fn completion_stream(&self, req: &CompletionRequest) -> Result<TaggedStream> {
        let mut last_err: Option<anyhow::Error> = None;
        for (i, provider) in self.providers.iter().enumerate() {
            match provider.completion_stream(req).await {
                Ok(s) => {
                    if i > 0 {
                        tracing::warn!(
                            provider = provider.name(),
                            model = provider.model(),
                            "[CHAIN] streaming fallback succeeded after {} primary failure(s)",
                            i,
                        );
                    }
                    return Ok(s);
                }
                Err(e) => {
                    tracing::warn!(
                        provider = provider.name(),
                        model = provider.model(),
                        error = %e,
                        "[CHAIN] streaming provider failed, trying next",
                    );
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow!("all providers in chain failed (stream)")))
    }
}
