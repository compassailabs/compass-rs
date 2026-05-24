use std::sync::Arc;

use alloy::primitives::Address;
use anyhow::{Result, anyhow};
use async_stream::stream;
use axum::response::sse::Event as SseEvent;
use chrono::{DateTime, Duration, Utc};
use futures::Stream;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::automation::chat_history::{ChatRole as DbChatRole, NewChatTurn};
use crate::automation::policy::{Policy, PolicyStatus};
use crate::core::llm::chat_engine::ToolTrace;
use crate::core::llm::provider::LlmProvider;
use crate::core::llm::skills::{RiskProfile, build_chat_system_prompt};
use crate::core::llm::sse_event::StreamEvent;
use crate::core::llm::stream::StreamFormat;
use crate::core::llm::stream_parser::{
    AnthropicAccumulator, OpenAiAccumulator, parse_anthropic_sse, parse_openai_sse,
};
use crate::core::llm::tool_context::ToolContext;
use crate::core::llm::types::{ChatMessage, CompletionRequest, ContentBlock, Role, ToolSchema};
use crate::state::AppState;

const MAX_TURNS: usize = 16;
const MAX_TOKENS: u32 = 2048;

fn chat_tools() -> Vec<ToolSchema> {
    vec![
        ToolSchema {
            name: "load_skill".into(),
            description:
                "Read the body of an on-demand skill reference by namespace key (e.g. \
                 'chat/policy_schema', 'chat/boundary', 'chat/policy_defaults'). Call \
                 BEFORE invoking a write tool you haven't used this conversation, or when \
                 you need details that aren't in the always-loaded module skill.md."
                    .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Skill key, e.g. chat/policy_schema" }
                },
                "required": ["name"]
            }),
        },
        ToolSchema {
            name: "check_balance".into(),
            description: "Read the user's Compass smart-account USDC balance on Arc and Base \
                          Sepolia. Use BEFORE proposing or committing a policy — if balance is \
                          zero on both chains, tell the user to fund their smart account first \
                          and STOP (do not call commit_policy). No args.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSchema {
            name: "read_market".into(),
            description: "Read the latest market snapshot (APRs, USDC peg, gas, gateway health). No args.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSchema {
            name: "read_position".into(),
            description: "Read the user's current on-chain allocation (Arc idle / Arbitrum idle / AAVE). Triggers a fresh RPC fetch. No args.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSchema {
            name: "read_policy".into(),
            description: "Read the user's active Policy. Returns the Policy JSON, or null if no policy is set yet.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSchema {
            name: "read_audit".into(),
            description: "Read recent automation decisions for the user. `since_unix_sec` defaults to 24h ago; `limit` defaults to 20.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "since_unix_sec": { "type": "integer" },
                    "limit": { "type": "integer" }
                }
            }),
        },
        ToolSchema {
            name: "commit_policy".into(),
            description: "Submit a Policy for the user. Server validates against schema and \
                          assigns the next version, returning `{ok: true, version: N}`. Use \
                          to create, update, or replace.\n\n\
                          HARD RULE: NEVER say 'Policy committed', 'Policy is now active', \
                          'I've set up your policy', 'Updating your policy now', or any \
                          equivalent without first emitting this tool_use block and quoting \
                          the version number from its real tool_result. There is no other \
                          valid pattern — server-side guards retract any reply that claims \
                          a commit without a matching tool call.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "policy": { "type": "object", "description": "Full Policy JSON per system-prompt schema." }
                },
                "required": ["policy"]
            }),
        },
        ToolSchema {
            name: "pause_policy".into(),
            description: "Pause the user's policy. Engine skips this user until resumed. \
                          HARD RULE: never narrate 'paused' / 'engine stopped' / 'I've \
                          paused your policy' without first emitting this tool_use and \
                          quoting the result.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolSchema {
            name: "resume_policy".into(),
            description: "Resume the user's paused policy. Engine starts ticking it again. \
                          HARD RULE: never narrate 'resumed' / 'engine running again' / \
                          'I've resumed your policy' without first emitting this tool_use \
                          and quoting the result.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        },
    ]
}

async fn dispatch_chat_tool(name: &str, args: &Value, ctx: &ToolContext) -> Result<String> {
    use crate::core::llm::tools::{market_tools, skill_tools};
    match name {
        "load_skill" => skill_tools::load_skill(args, ctx).await,
        "check_balance" => market_tools::check_balances(args, ctx).await,
        "read_market" => read_market(ctx).await,
        "read_position" => read_position(ctx).await,
        "read_policy" => read_policy(ctx).await,
        "read_audit" => read_audit(args, ctx).await,
        "commit_policy" => commit_policy(args, ctx).await,
        "pause_policy" => pause_policy(ctx).await,
        "resume_policy" => resume_policy(ctx).await,
        other => Ok(format!("Unknown tool: {other}")),
    }
}

async fn read_market(ctx: &ToolContext) -> Result<String> {
    match ctx.state.snapshots.latest().await? {
        Some(s) => Ok(serde_json::to_string(&s)?),
        None => Ok(json!({
            "snapshot": null,
            "note": "snapshot worker hasn't populated yet"
        })
        .to_string()),
    }
}

async fn read_position(ctx: &ToolContext) -> Result<String> {
    match ctx.state.position_fetcher.fetch(ctx.user).await {
        Ok(pos) => {
            let _ = ctx.state.positions.put(ctx.user, pos.clone()).await;
            Ok(serde_json::to_string(&pos)?)
        }
        Err(e) => match ctx.state.positions.get(ctx.user).await? {
            Some(pos) => Ok(serde_json::to_string(&pos)?),
            None => Ok(json!({
                "position": null,
                "note": format!("fetch failed: {e}; no cached value")
            })
            .to_string()),
        },
    }
}

async fn read_policy(ctx: &ToolContext) -> Result<String> {
    match ctx.state.policies.get(ctx.user).await? {
        Some(p) => Ok(serde_json::to_string(&p)?),
        None => Ok("null".into()),
    }
}

async fn read_audit(args: &Value, ctx: &ToolContext) -> Result<String> {
    let since = args
        .get("since_unix_sec")
        .and_then(|v| v.as_i64())
        .and_then(|s| DateTime::from_timestamp(s, 0))
        .or_else(|| Some(Utc::now() - Duration::hours(24)));
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize)
        .unwrap_or(20);
    let events = ctx.state.audit.list_for_user(ctx.user, since, limit).await?;
    Ok(serde_json::to_string(&events)?)
}

async fn commit_policy(args: &Value, ctx: &ToolContext) -> Result<String> {
    let policy_value = args
        .get("policy")
        .ok_or_else(|| anyhow!("missing 'policy' argument"))?;
    let mut policy: Policy = serde_json::from_value(policy_value.clone())
        .map_err(|e| anyhow!("policy JSON did not match schema: {e}"))?;
    policy.user = ctx.user;
    let version = ctx.state.policies.put(policy).await?;

    let state = ctx.state.clone();
    let user = ctx.user;
    tokio::spawn(async move {
        crate::automation::scheduler::tick_user_now(&state, user).await;
    });

    Ok(json!({ "ok": true, "version": version }).to_string())
}

async fn pause_policy(ctx: &ToolContext) -> Result<String> {
    ctx.state
        .policies
        .set_status(ctx.user, PolicyStatus::Paused)
        .await?;
    Ok(json!({ "ok": true, "status": "paused" }).to_string())
}

async fn resume_policy(ctx: &ToolContext) -> Result<String> {
    ctx.state
        .policies
        .set_status(ctx.user, PolicyStatus::Active)
        .await?;
    let state = ctx.state.clone();
    let user = ctx.user;
    tokio::spawn(async move {
        crate::automation::scheduler::tick_user_now(&state, user).await;
    });
    Ok(json!({ "ok": true, "status": "active" }).to_string())
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatTurn {
    pub role: ChatRole,
    pub text: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct ChatResult {
    pub model: String,
    pub turns: usize,
    pub reply: String,
    pub trace: Vec<ToolTrace>,
}

pub async fn run_chat_agent(
    llm: &dyn LlmProvider,
    ctx: &ToolContext,
    history: &[ChatTurn],
    new_message: &str,
    live_state: Option<&str>,
) -> Result<ChatResult> {
    let tools = chat_tools();
    let system = build_chat_system_prompt(live_state);

    let mut messages: Vec<ChatMessage> = history
        .iter()
        .map(|t| ChatMessage {
            role: match t.role {
                ChatRole::Assistant => Role::Assistant,
                ChatRole::User => Role::User,
            },
            content: vec![ContentBlock::Text { text: t.text.clone() }],
        })
        .collect();
    messages.push(ChatMessage {
        role: Role::User,
        content: vec![ContentBlock::Text {
            text: new_message.to_string(),
        }],
    });

    let mut trace = Vec::new();

    for turn in 0..MAX_TURNS {
        let req = CompletionRequest {
            system: system.clone(),
            messages: messages.clone(),
            tools: tools.clone(),
            max_tokens: MAX_TOKENS,
            temperature: None,
        };
        let resp = llm.complete(&req).await?;

        messages.push(ChatMessage {
            role: Role::Assistant,
            content: resp.content.clone(),
        });

        let mut this_turn_text = String::new();
        let mut tool_results: Vec<ContentBlock> = Vec::new();

        for block in &resp.content {
            match block {
                ContentBlock::Text { text } => {
                    if !this_turn_text.is_empty() {
                        this_turn_text.push_str("\n\n");
                    }
                    this_turn_text.push_str(text);
                }
                ContentBlock::ToolUse { id, name, input } => {
                    let output = match dispatch_chat_tool(name, input, ctx).await {
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
                ContentBlock::ToolResult { .. } => {}
            }
        }

        if tool_results.is_empty() {
            return Ok(ChatResult {
                model: llm.model().to_string(),
                turns: turn + 1,
                reply: this_turn_text,
                trace,
            });
        }

        messages.push(ChatMessage {
            role: Role::User,
            content: tool_results,
        });
    }

    Err(anyhow!(
        "chat agent exceeded MAX_TURNS ({MAX_TURNS}) without finishing"
    ))
}

pub fn stream_chat_agent(
    llm: Arc<dyn LlmProvider>,
    state: AppState,
    user: Address,
    risk: RiskProfile,
    history: Vec<ChatTurn>,
    new_message: String,
    live_state: Option<String>,
) -> impl Stream<Item = SseEvent> + Send {
    stream! {
        let tools = chat_tools();
        let system = build_chat_system_prompt(live_state.as_deref());
        let ctx = ToolContext::new(state.clone(), user, risk);

        let mut messages: Vec<ChatMessage> = history
            .into_iter()
            .map(|t| ChatMessage {
                role: match t.role {
                    ChatRole::Assistant => Role::Assistant,
                    ChatRole::User => Role::User,
                },
                content: vec![ContentBlock::Text { text: t.text }],
            })
            .collect();
        messages.push(ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text { text: new_message }],
        });

        let mut full_assistant_text = String::new();
        let mut trace: Vec<ToolTrace> = Vec::new();

        for turn in 0..MAX_TURNS {
            let req = CompletionRequest {
                system: system.clone(),
                messages: messages.clone(),
                tools: tools.clone(),
                max_tokens: MAX_TOKENS,
                temperature: None,
            };

            let tagged = match llm.completion_stream(&req).await {
                Ok(t) => t,
                Err(e) => {
                    yield StreamEvent::Error { message: e.to_string() }.into_sse_event();
                    yield StreamEvent::MessageStop {
                        stop_reason: "error".into(),
                    }.into_sse_event();
                    return;
                }
            };

            let format = tagged.format;
            let mut bytes_stream = tagged.stream;
            let mut buffer = String::new();
            let mut openai_acc = OpenAiAccumulator::new();
            let mut anthropic_acc = AnthropicAccumulator::new();
            let mut this_turn_tools: Vec<crate::core::llm::stream::ParsedToolCall> = Vec::new();
            let mut this_turn_text = String::new();

            while let Some(chunk_res) = bytes_stream.next().await {
                let chunk = match chunk_res {
                    Ok(b) => b,
                    Err(e) => {
                        yield StreamEvent::Error {
                            message: format!("stream read failed: {e}"),
                        }.into_sse_event();
                        yield StreamEvent::MessageStop {
                            stop_reason: "error".into(),
                        }.into_sse_event();
                        return;
                    }
                };
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                let (events, tool_calls) = match format {
                    StreamFormat::OpenAiSse => parse_openai_sse(&mut buffer, &mut openai_acc),
                    StreamFormat::AnthropicSse => {
                        parse_anthropic_sse(&mut buffer, &mut anthropic_acc)
                    }
                };

                for ev in events {
                    if let StreamEvent::TextDelta { text } = &ev {
                        this_turn_text.push_str(text);
                    }
                    yield ev.into_sse_event();
                }
                this_turn_tools.extend(tool_calls);
            }

            if this_turn_tools.is_empty() {
                let fabricated = detect_fabricated_action(&this_turn_text, &trace).is_some();

                if fabricated && turn + 1 < MAX_TURNS {
                    tracing::warn!(
                        turn,
                        text_snippet = %this_turn_text.chars().take(160).collect::<String>(),
                        "[CHAT] fabrication detected — injecting corrective and retrying"
                    );
                    yield StreamEvent::TextReplace {
                        text: "(let me actually run that…)".into(),
                    }.into_sse_event();
                    messages.push(ChatMessage {
                        role: Role::Assistant,
                        content: vec![ContentBlock::Text {
                            text: this_turn_text.clone(),
                        }],
                    });
                    messages.push(ChatMessage {
                        role: Role::User,
                        content: vec![ContentBlock::Text {
                            text: "SYSTEM CHECK: Your last reply asserted a write action \
                                   (committed / paused / resumed / set up a policy) but you \
                                   did NOT emit the corresponding tool_use block — so nothing \
                                   was actually changed. The server has retracted that reply. \
                                   Either: (a) emit the tool_use now and let me execute it, \
                                   or (b) reply plainly that you can't act yet and ask the \
                                   user for whatever info you're missing. Do NOT re-narrate \
                                   the fake confirmation."
                                .into(),
                        }],
                    });
                    continue;
                }

                full_assistant_text.push_str(&this_turn_text);
                if fabricated {
                    if let Some(retraction) =
                        detect_fabricated_action(&full_assistant_text, &trace)
                    {
                        yield StreamEvent::TextReplace {
                            text: retraction.clone(),
                        }.into_sse_event();
                        full_assistant_text = retraction;
                    }
                }
                yield StreamEvent::MessageStop {
                    stop_reason: "end_turn".into(),
                }.into_sse_event();
                persist_assistant_turn(&state, user, &full_assistant_text, &trace).await;
                return;
            }

            full_assistant_text.push_str(&this_turn_text);

            let mut assistant_blocks: Vec<ContentBlock> = Vec::new();
            if !this_turn_text.is_empty() {
                assistant_blocks.push(ContentBlock::Text {
                    text: this_turn_text.clone(),
                });
            }
            for tc in &this_turn_tools {
                assistant_blocks.push(ContentBlock::ToolUse {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input: tc.arguments.clone(),
                });
            }
            messages.push(ChatMessage {
                role: Role::Assistant,
                content: assistant_blocks,
            });

            let mut tool_result_blocks: Vec<ContentBlock> = Vec::new();
            for tc in this_turn_tools.into_iter() {
                yield StreamEvent::ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input: tc.arguments.clone(),
                }.into_sse_event();

                let output = match dispatch_chat_tool(&tc.name, &tc.arguments, &ctx).await {
                    Ok(s) => s,
                    Err(e) => format!("ERROR: {e}"),
                };
                trace.push(ToolTrace {
                    turn,
                    name: tc.name.clone(),
                    input: tc.arguments.clone(),
                    output: output.clone(),
                });

                yield StreamEvent::ToolResult {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    output: output.clone(),
                }.into_sse_event();

                tool_result_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: tc.id,
                    content: output,
                });
            }

            messages.push(ChatMessage {
                role: Role::User,
                content: tool_result_blocks,
            });
        }

        yield StreamEvent::Error {
            message: format!("chat agent exceeded MAX_TURNS ({MAX_TURNS})"),
        }.into_sse_event();
        yield StreamEvent::MessageStop {
            stop_reason: "max_turns".into(),
        }.into_sse_event();
        persist_assistant_turn(&state, user, &full_assistant_text, &trace).await;
    }
}

const WRITE_TOOLS: &[&str] = &["commit_policy", "pause_policy", "resume_policy"];

const FABRICATION_PHRASES: &[&str] = &[
    "policy committed",
    "policy is live",
    "policy is now active",
    "policy is now live",
    "policy is active",
    "your policy is now",
    "your balanced policy is",
    "your conservative policy is",
    "your growth policy is",
    "i've committed",
    "i have committed",
    "i've set up",
    "i have set up",
    "i've configured",
    "i have configured",
    "i've updated",
    "i have updated",
    "i've changed",
    "i have changed",
    "has been committed",
    "now committed",
    "policy committed ✅",
    "✅ — balanced",
    "✅ — conservative",
    "✅ — growth",
    "updating your policy now",
    "i'm committing",
    "i am committing",
];

fn detect_fabricated_action(text: &str, trace: &[ToolTrace]) -> Option<String> {
    let any_write_called = trace.iter().any(|t| WRITE_TOOLS.contains(&t.name.as_str()));
    if any_write_called {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let matched: Vec<&str> = FABRICATION_PHRASES
        .iter()
        .copied()
        .filter(|p| lower.contains(p))
        .collect();
    if matched.is_empty() {
        return None;
    }

    tracing::warn!(
        matched = ?matched,
        text_snippet = %text.chars().take(180).collect::<String>(),
        "[CHAT] fabrication detector tripped — replacing claimed confirmation"
    );

    Some(format!(
        "⚠ Internal guard: my previous reply claimed I committed a policy change, \
         but no `commit_policy` tool call was actually dispatched this turn — so \
         **nothing was changed**. This is a safety check; the response was \
         retracted before it was saved.\n\n\
         If you want me to actually commit the policy, please tell me again and \
         I'll call the tool this time."
    ))
}

async fn persist_assistant_turn(
    state: &AppState,
    user: Address,
    text: &str,
    trace: &[ToolTrace],
) {
    let trace_json = serde_json::to_value(trace).ok();
    if let Err(e) = state
        .chat_history
        .append(NewChatTurn {
            user,
            role: DbChatRole::Assistant,
            text: text.to_string(),
            trace: trace_json,
        })
        .await
    {
        tracing::warn!(user = %user, error = %e, "failed to persist assistant turn (stream)");
    }
}
