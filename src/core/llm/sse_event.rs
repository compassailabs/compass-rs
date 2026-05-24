use axum::response::sse::Event;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },

    #[serde(rename = "thinking_delta")]
    ThinkingDelta { text: String },

    #[serde(rename = "tool_call")]
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        id: String,
        name: String,
        output: String,
    },

    #[serde(rename = "text_replace")]
    TextReplace { text: String },

    #[serde(rename = "message_stop")]
    MessageStop { stop_reason: String },

    #[serde(rename = "error")]
    Error { message: String },
}

impl StreamEvent {
    fn event_name(&self) -> &'static str {
        match self {
            StreamEvent::TextDelta { .. } => "text_delta",
            StreamEvent::ThinkingDelta { .. } => "thinking_delta",
            StreamEvent::ToolCall { .. } => "tool_call",
            StreamEvent::ToolResult { .. } => "tool_result",
            StreamEvent::TextReplace { .. } => "text_replace",
            StreamEvent::MessageStop { .. } => "message_stop",
            StreamEvent::Error { .. } => "error",
        }
    }

    pub fn into_sse_event(self) -> Event {
        let name = self.event_name();
        let json = serde_json::to_string(&self)
            .unwrap_or_else(|_| "{\"type\":\"error\",\"message\":\"serialization failed\"}".into());
        Event::default().event(name).data(json)
    }
}
