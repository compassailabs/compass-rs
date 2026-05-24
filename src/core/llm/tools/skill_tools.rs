use anyhow::Result;
use serde_json::Value;

use crate::core::llm::skills::load_on_demand;
use crate::core::llm::tool_context::ToolContext;

pub async fn load_skill(args: &Value, _ctx: &ToolContext) -> Result<String> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing skill name"))?;
    match load_on_demand(name) {
        Some(body) => Ok(body.to_string()),
        None => Ok(format!(
            "No skill named '{name}'. Known keys are `<module>/<file-stem>` where module ∈ \
             {{core, chat, onchain, setup}}. See `core/skill.md`, `chat/skill.md`, etc. \
             for each module's `When to Load References` table."
        )),
    }
}
