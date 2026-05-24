use serde_json::Value;
use uuid::Uuid;

use super::sse_event::StreamEvent;
use super::stream::ParsedToolCall;

pub struct OpenAiAccumulator {
    partial_tools: Vec<PartialToolCall>,
    pending_text: String,
    in_think_block: bool,
}

struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

impl OpenAiAccumulator {
    pub fn new() -> Self {
        Self {
            partial_tools: Vec::new(),
            pending_text: String::new(),
            in_think_block: false,
        }
    }

    fn flush_tools(&mut self) -> Vec<ParsedToolCall> {
        self.partial_tools
            .drain(..)
            .filter(|pt| !pt.name.is_empty())
            .map(|pt| ParsedToolCall {
                id: pt.id,
                name: pt.name,
                arguments: serde_json::from_str(&pt.arguments).unwrap_or(Value::Null),
            })
            .collect()
    }
}

pub fn parse_openai_sse(
    buffer: &mut String,
    accumulator: &mut OpenAiAccumulator,
) -> (Vec<StreamEvent>, Vec<ParsedToolCall>) {
    let mut events = Vec::new();
    let mut tool_calls = Vec::new();

    while let Some(pos) = buffer.find("\n\n") {
        let block: String = buffer[..pos].to_string();
        *buffer = buffer[pos + 2..].to_string();

        for line in block.lines() {
            let line = line.trim();
            if !line.starts_with("data: ") {
                continue;
            }
            let data = &line[6..];
            if data == "[DONE]" {
                tool_calls.extend(accumulator.flush_tools());
                return (events, tool_calls);
            }
            let Ok(json) = serde_json::from_str::<Value>(data) else {
                tracing::debug!("[OPENAI PARSE] invalid JSON: {data:?}");
                continue;
            };

            let Some(choice) = json
                .get("choices")
                .and_then(|c| c.as_array())
                .and_then(|c| c.first())
            else {
                continue;
            };
            let delta = choice.get("delta").unwrap_or(&Value::Null);
            let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());

            if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                if !reasoning.is_empty() {
                    events.push(StreamEvent::ThinkingDelta {
                        text: reasoning.to_string(),
                    });
                }
            }

            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    split_inline_think(content, accumulator, &mut events);
                }
            }

            if let Some(tc_deltas) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                for tc_delta in tc_deltas {
                    let idx = tc_delta
                        .get("index")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as usize;
                    while accumulator.partial_tools.len() <= idx {
                        accumulator.partial_tools.push(PartialToolCall {
                            id: String::new(),
                            name: String::new(),
                            arguments: String::new(),
                        });
                    }
                    let slot = &mut accumulator.partial_tools[idx];
                    if let Some(id) = tc_delta.get("id").and_then(|v| v.as_str()) {
                        slot.id = id.to_string();
                    }
                    if let Some(func) = tc_delta.get("function") {
                        if let Some(name) = func.get("name").and_then(|v| v.as_str()) {
                            slot.name = name.to_string();
                        }
                        if let Some(args) = func.get("arguments").and_then(|v| v.as_str()) {
                            slot.arguments.push_str(args);
                        }
                    }
                    if slot.id.is_empty() && !slot.name.is_empty() {
                        slot.id = format!("call_{}", Uuid::new_v4().simple());
                    }
                }
            }

            if matches!(finish_reason, Some("tool_calls") | Some("stop") | Some("length")) {
                if !accumulator.pending_text.is_empty() {
                    let text = std::mem::take(&mut accumulator.pending_text);
                    events.push(if accumulator.in_think_block {
                        StreamEvent::ThinkingDelta { text }
                    } else {
                        StreamEvent::TextDelta { text }
                    });
                }
                tool_calls.extend(accumulator.flush_tools());
            }
        }
    }

    (events, tool_calls)
}

fn split_inline_think(
    chunk: &str,
    acc: &mut OpenAiAccumulator,
    out: &mut Vec<StreamEvent>,
) {
    let mut buf = std::mem::take(&mut acc.pending_text);
    buf.push_str(chunk);

    loop {
        let marker = if acc.in_think_block { "</think>" } else { "<think>" };
        if let Some(idx) = buf.find(marker) {
            let before = buf[..idx].to_string();
            if !before.is_empty() {
                out.push(if acc.in_think_block {
                    StreamEvent::ThinkingDelta { text: before }
                } else {
                    StreamEvent::TextDelta { text: before }
                });
            }
            buf = buf[idx + marker.len()..].to_string();
            acc.in_think_block = !acc.in_think_block;
            continue;
        }

        let probe_marker = if acc.in_think_block { "</think>" } else { "<think>" };
        let mut hold = 0;
        for n in (1..probe_marker.len()).rev() {
            if buf.len() >= n && buf.is_char_boundary(buf.len() - n)
                && probe_marker.starts_with(&buf[buf.len() - n..])
            {
                hold = n;
                break;
            }
        }
        if hold > 0 {
            let cut = buf.len() - hold;
            let tail = buf[cut..].to_string();
            let head = buf[..cut].to_string();
            if !head.is_empty() {
                out.push(if acc.in_think_block {
                    StreamEvent::ThinkingDelta { text: head }
                } else {
                    StreamEvent::TextDelta { text: head }
                });
            }
            acc.pending_text = tail;
        } else if !buf.is_empty() {
            out.push(if acc.in_think_block {
                StreamEvent::ThinkingDelta { text: buf }
            } else {
                StreamEvent::TextDelta { text: buf }
            });
        }
        break;
    }
}

pub struct AnthropicAccumulator {
    current_block: Option<AnthropicBlockType>,
    tool_calls: Vec<PartialToolCall>,
}

enum AnthropicBlockType {
    Text,
    Thinking,
    ToolUse { id: String, name: String, arguments: String },
}

impl AnthropicAccumulator {
    pub fn new() -> Self {
        Self {
            current_block: None,
            tool_calls: Vec::new(),
        }
    }

    fn flush_tools(&mut self) -> Vec<ParsedToolCall> {
        self.tool_calls
            .drain(..)
            .filter(|pt| !pt.name.is_empty())
            .map(|pt| ParsedToolCall {
                id: pt.id,
                name: pt.name,
                arguments: serde_json::from_str(&pt.arguments).unwrap_or(Value::Null),
            })
            .collect()
    }
}

pub fn parse_anthropic_sse(
    buffer: &mut String,
    accumulator: &mut AnthropicAccumulator,
) -> (Vec<StreamEvent>, Vec<ParsedToolCall>) {
    let mut events = Vec::new();
    let mut tool_calls = Vec::new();

    while let Some(pos) = buffer.find("\n\n") {
        let block: String = buffer[..pos].to_string();
        *buffer = buffer[pos + 2..].to_string();

        let mut event_type = "";
        let mut data_str = String::new();
        for line in block.lines() {
            let line = line.trim();
            if let Some(et) = line.strip_prefix("event: ") {
                event_type = et.trim();
            } else if let Some(d) = line.strip_prefix("data: ") {
                data_str = d.to_string();
            }
        }
        if data_str.is_empty() {
            continue;
        }
        let Ok(data) = serde_json::from_str::<Value>(&data_str) else {
            tracing::debug!("[ANTHROPIC PARSE] invalid JSON: {data_str:?}");
            continue;
        };

        match event_type {
            "content_block_start" => {
                let block_type = data
                    .get("content_block")
                    .and_then(|b| b.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                match block_type {
                    "text" => accumulator.current_block = Some(AnthropicBlockType::Text),
                    "thinking" => accumulator.current_block = Some(AnthropicBlockType::Thinking),
                    "tool_use" => {
                        let cb = data.get("content_block");
                        let name = cb
                            .and_then(|b| b.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        let id = cb
                            .and_then(|b| b.get("id"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        accumulator.current_block = Some(AnthropicBlockType::ToolUse {
                            id,
                            name,
                            arguments: String::new(),
                        });
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let delta = data.get("delta").unwrap_or(&Value::Null);
                let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
                match delta_type {
                    "text_delta" => {
                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                events.push(StreamEvent::TextDelta {
                                    text: text.to_string(),
                                });
                            }
                        }
                    }
                    "thinking_delta" => {
                        if let Some(text) = delta.get("thinking").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                events.push(StreamEvent::ThinkingDelta {
                                    text: text.to_string(),
                                });
                            }
                        }
                    }
                    "input_json_delta" => {
                        if let Some(json_str) =
                            delta.get("partial_json").and_then(|t| t.as_str())
                        {
                            if let Some(AnthropicBlockType::ToolUse {
                                ref mut arguments, ..
                            }) = accumulator.current_block
                            {
                                arguments.push_str(json_str);
                            }
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some(AnthropicBlockType::ToolUse {
                    id,
                    name,
                    arguments,
                }) = accumulator.current_block.take()
                {
                    accumulator.tool_calls.push(PartialToolCall {
                        id,
                        name,
                        arguments,
                    });
                }
            }
            "message_delta" => {
                let reason = data
                    .get("delta")
                    .and_then(|d| d.get("stop_reason"))
                    .and_then(|r| r.as_str());
                if matches!(reason, Some("tool_use") | Some("end_turn") | Some("max_tokens")) {
                    tool_calls.extend(accumulator.flush_tools());
                }
            }
            "message_stop" => {
                tool_calls.extend(accumulator.flush_tools());
            }
            _ => {}
        }
    }

    (events, tool_calls)
}
