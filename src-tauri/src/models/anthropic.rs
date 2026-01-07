use crate::protocol::ChatRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(flatten)]
    pub data: Value,
}

#[derive(Serialize)]
#[allow(dead_code)]
pub struct AnthropicRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<AnthropicMessage>,
    pub stream: bool,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
}

#[derive(Serialize, Clone)]
#[allow(dead_code)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

#[derive(Serialize, Clone)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Serialize, Clone)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[allow(dead_code)]
pub fn convert_to_anthropic(openai_req: &ChatRequest) -> AnthropicRequest {
    let mut system = None;
    let mut messages = Vec::new();

    for msg in &openai_req.messages {
        match msg.role.as_str() {
            "system" => {
                // Extract system message
                if let Some(content) = &msg.content {
                    system = Some(content.clone());
                }
            }
            "assistant" => {
                // Handle assistant messages with tool calls
                if let Some(tool_calls) = &msg.tool_calls {
                    let mut blocks = Vec::new();

                    // Add text content if present
                    if let Some(content) = &msg.content {
                        if !content.is_empty() {
                            blocks.push(AnthropicContentBlock::Text {
                                text: content.clone(),
                            });
                        }
                    }

                    // Add tool use blocks
                    for call in tool_calls {
                        let input: Value = serde_json::from_str(&call.function.arguments)
                            .unwrap_or(Value::Object(serde_json::Map::new()));

                        blocks.push(AnthropicContentBlock::ToolUse {
                            id: call.id.clone(),
                            name: call.function.name.clone(),
                            input,
                        });
                    }

                    messages.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: AnthropicContent::Blocks(blocks),
                    });
                } else if let Some(content) = &msg.content {
                    // Regular assistant message
                    messages.push(AnthropicMessage {
                        role: "assistant".to_string(),
                        content: AnthropicContent::Text(content.clone()),
                    });
                }
            }
            "tool" => {
                // Convert tool results to user message with tool_result blocks
                if let Some(tool_call_id) = &msg.tool_call_id {
                    if let Some(content) = &msg.content {
                        // Tool results go in a user message
                        messages.push(AnthropicMessage {
                            role: "user".to_string(),
                            content: AnthropicContent::Blocks(vec![
                                AnthropicContentBlock::ToolResult {
                                    tool_use_id: tool_call_id.clone(),
                                    content: content.clone(),
                                },
                            ]),
                        });
                    }
                }
            }
            "user" => {
                if let Some(content) = &msg.content {
                    messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: AnthropicContent::Text(content.clone()),
                    });
                }
            }
            _ => {}
        }
    }

    // Convert OpenAI tool format to Anthropic tool format
    // OpenAI: {"type": "function", "function": {"name": "...", "description": "...", "parameters": {...}}}
    // Anthropic: {"name": "...", "description": "...", "input_schema": {...}}
    let anthropic_tools = openai_req.tools.as_ref().map(|tools| {
        tools
            .iter()
            .filter_map(|tool| {
                // Extract the nested function object
                let function = tool.get("function")?.as_object()?;

                let mut anthropic_tool = serde_json::Map::new();

                // Required: name
                if let Some(name) = function.get("name") {
                    anthropic_tool.insert("name".to_string(), name.clone());
                } else {
                    eprintln!("WARNING: Tool missing name field");
                    return None;
                }

                // Optional: description (but recommended)
                if let Some(description) = function.get("description") {
                    anthropic_tool.insert("description".to_string(), description.clone());
                } else {
                    // Provide empty description if missing
                    anthropic_tool.insert("description".to_string(), Value::String(String::new()));
                }

                // Required: input_schema (converted from parameters)
                if let Some(parameters) = function.get("parameters") {
                    anthropic_tool.insert("input_schema".to_string(), parameters.clone());
                } else {
                    eprintln!("WARNING: Tool missing parameters field");
                    return None;
                }

                Some(Value::Object(anthropic_tool))
            })
            .collect()
    });

    // Set tool_choice to "any" when tools are present to FORCE native tool calling
    // This prevents Claude from outputting XML descriptions instead of using the API
    let tool_choice = if anthropic_tools.is_some() {
        Some(serde_json::json!({"type": "any"}))
    } else {
        None
    };

    AnthropicRequest {
        model: openai_req.model.clone(),
        system,
        messages,
        stream: openai_req.stream,
        max_tokens: 4096,
        tools: anthropic_tools,
        tool_choice,
    }
}
