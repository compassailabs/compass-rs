use std::env;
use std::sync::Arc;

use super::anthropic::AnthropicProvider;
use super::chain::ProviderChain;
use super::openai_compatible::OpenAiCompatibleProvider;
use super::provider::LlmProvider;

pub fn build_llm_from_env() -> Arc<dyn LlmProvider> {
    let mut providers: Vec<Arc<dyn LlmProvider>> = Vec::new();

    if let Some(key) = nonempty_env("DEEPSEEK_API_KEY") {
        let base_url = env::var("DEEPSEEK_BASE_URL")
            .unwrap_or_else(|_| "https://api.deepseek.com/v1".into());
        let model =
            env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-chat".into());
        tracing::info!(provider = "deepseek", %model, %base_url, "[LLM] adding to chain (primary)");
        providers.push(Arc::new(OpenAiCompatibleProvider::new(
            "deepseek", &base_url, &model, &key,
        )));
    }

    if let Some(key) = nonempty_env("OPENAI_API_KEY") {
        let base_url =
            env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into());
        let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
        tracing::info!(provider = "openai", %model, %base_url, "[LLM] adding to chain (fallback)");
        providers.push(Arc::new(OpenAiCompatibleProvider::new(
            "openai", &base_url, &model, &key,
        )));
    }

    if let Some(key) = nonempty_env("ANTHROPIC_API_KEY") {
        let model =
            env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-sonnet-4-6".into());
        tracing::info!(provider = "anthropic", %model, "[LLM] adding to chain (final fallback)");
        providers.push(Arc::new(AnthropicProvider::new(&model, &key)));
    }

    match providers.len() {
        0 => {
            tracing::warn!(
                "[LLM] no provider keys configured (DEEPSEEK_API_KEY / OPENAI_API_KEY / \
                 ANTHROPIC_API_KEY all unset) — chat endpoints will return errors",
            );
            Arc::new(AnthropicProvider::new("claude-sonnet-4-6", ""))
        }
        1 => providers.pop().unwrap(),
        _ => Arc::new(ProviderChain::new(providers)),
    }
}

fn nonempty_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|s| !s.trim().is_empty())
}
