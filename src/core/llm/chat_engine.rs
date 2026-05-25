use anyhow::{Result, anyhow};
use serde::Serialize;

use super::provider::LlmProvider;
use super::skills::{RiskProfile, build_system_prompt};
use super::tool_context::ToolContext;
use super::tool_dispatch::execute_tool;
use super::tools::registry as tool_registry;
use super::types::{ChatMessage, CompletionRequest, ContentBlock, Role};

const MAX_TURNS: usize = 8;
const MAX_TOKENS: u32 = 2048;

#[derive(Serialize, Debug, Clone)]
pub struct ToolTrace {
    pub turn: usize,
    pub name: String,
    pub input: serde_json::Value,
    pub output: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct AgentResult {
    pub model: String,
    pub turns: usize,
    pub final_text: String,
    pub trace: Vec<ToolTrace>,
}

pub async fn run_agent(
    llm: &dyn LlmProvider,
    ctx: &ToolContext,
    risk: RiskProfile,
    user_message: &str,
    state_summary: Option<&str>,
) -> Result<AgentResult> {
    let system = build_system_prompt(risk, state_summary);
    let tools = tool_registry();

    let mut messages: Vec<ChatMessage> = vec![ChatMessage {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: user_message.to_string(),
        }],
    }];
    let mut trace = Vec::new();
    let mut final_text = String::new();

    for turn in 0..MAX_TURNS {
        let req = CompletionRequest {
            system: system.clone(),
            messages: messages.clone(),
            tools: tools.clone(),
            max_tokens: MAX_TOKENS,
            temperature: None,
            thinking_budget_tokens: None,
        };
        let resp = llm.complete(&req).await?;

        messages.push(ChatMessage {
            role: Role::Assistant,
            content: resp.content.clone(),
        });

        let mut tool_results: Vec<ContentBlock> = Vec::new();
        for block in &resp.content {
            match block {
                ContentBlock::Text { text } => {
                    if !final_text.is_empty() {
                        final_text.push_str("\n\n");
                    }
                    final_text.push_str(text);
                }
                ContentBlock::ToolUse { id, name, input } => {
                    let output = match execute_tool(name, input, ctx).await {
                        Ok(s) => s,
                        Err(e) => format!("ERROR: {e}"),
                    };
                    trace.push(ToolTrace {
                        turn,
                        name: name.clone(),
                        input: input.clone(),
                        output: output.clone(),
                    });
                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: output,
                    });
                }
                ContentBlock::ToolResult { .. } | ContentBlock::Thinking { .. } => {}
            }
        }

        if tool_results.is_empty() {
            return Ok(AgentResult {
                model: llm.model().to_string(),
                turns: turn + 1,
                final_text,
                trace,
            });
        }

        messages.push(ChatMessage {
            role: Role::User,
            content: tool_results,
        });
    }

    Err(anyhow!("agent exceeded MAX_TURNS ({MAX_TURNS}) without finishing"))
}
