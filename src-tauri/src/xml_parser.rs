/// Parse XML-like tool call descriptions from Claude models
/// These appear when Claude describes what it wants to do rather than making actual tool calls

pub struct XmlToolCall {
    pub name: String,
    pub parameters: Vec<(String, String)>,
}

pub fn detect_xml_tool_calls(text: &str) -> Option<Vec<XmlToolCall>> {
    if !text.contains("<function_calls>")
        && !text.contains("<invoke")
        && !text.contains("<tool_call>")
        && !text.contains("<function=")
    {
        return None;
    }

    let mut calls = Vec::new();

    // First try Qwen Code format: <tool_call><function=NAME><parameter=KEY>value</parameter></function></tool_call>
    let mut pos = 0;
    while let Some(tc_start) = text[pos..].find("<tool_call>") {
        let tc_start = pos + tc_start + 11;
        if let Some(tc_end) = text[tc_start..].find("</tool_call>") {
            let tc_end = tc_start + tc_end;
            let content = &text[tc_start..tc_end];

            // Look for <function=NAME>
            if let Some(func_start) = content.find("<function=") {
                let func_start = func_start + 10;
                if let Some(func_end) = content[func_start..].find('>') {
                    let func_end = func_start + func_end;
                    let name = content[func_start..func_end].trim().to_string();
                    eprintln!("[XML] Found Qwen function: {}", name);

                    let mut parameters = Vec::new();
                    let mut param_pos = func_end;

                    // Look for <parameter=KEY>value</parameter>
                    while let Some(p_start) = content[param_pos..].find("<parameter=") {
                        let p_start = param_pos + p_start + 11;
                        if let Some(p_key_end) = content[p_start..].find('>') {
                            let p_key_end = p_start + p_key_end;
                            let key = content[p_start..p_key_end].trim().to_string();

                            let val_start = p_key_end + 1;
                            if let Some(val_end) = content[val_start..].find("</parameter>") {
                                let val_end = val_start + val_end;
                                let value = content[val_start..val_end].trim().to_string();
                                eprintln!("[XML] Found Qwen param: {}={}", key, value);
                                parameters.push((key, value));
                                param_pos = val_end + 12;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    }

                    if !name.is_empty() {
                        calls.push(XmlToolCall { name, parameters });
                    }
                }
            }

            pos = tc_end + 12;
        } else {
            break;
        }
    }

    if !calls.is_empty() {
        return Some(calls);
    }

    // Fallback to <invoke name="..."> blocks
    let mut pos = 0;
    while let Some(invoke_start) = text[pos..].find("<invoke") {
        let invoke_start = pos + invoke_start;

        // Find the name attribute
        if let Some(name_start) = text[invoke_start..].find("name=\"") {
            let name_start = invoke_start + name_start + 6;
            if let Some(name_end) = text[name_start..].find('"') {
                let name_end = name_start + name_end;
                let name = text[name_start..name_end].to_string();

                // Find parameters
                let mut parameters = Vec::new();
                let mut param_pos = name_end;

                while let Some(param_start) = text[param_pos..].find("<parameter name=\"") {
                    let param_start = param_pos + param_start + 17;
                    if let Some(param_name_end) = text[param_start..].find('"') {
                        let param_name_end = param_start + param_name_end;
                        let param_name = text[param_start..param_name_end].to_string();

                        // Find the parameter value (between > and </parameter>)
                        if let Some(value_start) = text[param_name_end..].find('>') {
                            let value_start = param_name_end + value_start + 1;
                            if let Some(value_end) = text[value_start..].find("</parameter>") {
                                let value_end = value_start + value_end;
                                let param_value = text[value_start..value_end].to_string();
                                parameters.push((param_name, param_value));
                                param_pos = value_end;
                            } else {
                                break;
                            }
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }

                    // Stop if we've reached the end of this invoke
                    if let Some(invoke_end) = text[param_pos..].find("</invoke>") {
                        if let Some(next_invoke) = text[param_pos..].find("<invoke") {
                            if invoke_end < next_invoke {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }

                calls.push(XmlToolCall { name, parameters });
            }
        }

        // Move past this invoke
        if let Some(invoke_end) = text[invoke_start..].find("</invoke>") {
            pos = invoke_start + invoke_end + 9;
        } else {
            break;
        }
    }

    if calls.is_empty() {
        // Try parsing GLM/MiniMax/Qwen format: <tool_call>{"name": "...", "arguments": {...}}</tool_call>
        // or: <tool_call>name<arg_key>key</arg_key><arg_value>value</arg_value></tool_call>
        let mut pos = 0;
        while let Some(start) = text[pos..].find("<tool_call>") {
            eprintln!("[XML] Found <tool_call> at position {}", pos + start);
            let start = pos + start + 11;
            if let Some(end) = text[start..].find("</tool_call>") {
                let end = start + end;
                let content = &text[start..end];
                eprintln!("[XML] Tool call content: {}", content);

                // Try parsing as JSON first (Qwen format)
                if content.trim().starts_with('{') {
                    eprintln!("[XML] Attempting JSON parse for Qwen format");
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(content.trim()) {
                        if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
                            let mut parameters = Vec::new();
                            if let Some(args) = json.get("arguments").and_then(|v| v.as_object()) {
                                for (key, value) in args {
                                    let value_str = match value {
                                        serde_json::Value::String(s) => s.clone(),
                                        other => other.to_string(),
                                    };
                                    parameters.push((key.clone(), value_str));
                                }
                            }
                            eprintln!(
                                "[XML] Parsed Qwen JSON tool call: name={}, params={}",
                                name,
                                parameters.len()
                            );
                            calls.push(XmlToolCall {
                                name: name.to_string(),
                                parameters,
                            });
                            pos = end + 12;
                            continue;
                        }
                    }
                }

                eprintln!("[XML] Attempting GLM/MiniMax format parse");

                // Content starts with function name, then args
                // Example: read_file<arg_key>path</arg_key><arg_value>...</arg_value>

                let name_end = content.find("<arg_key>").unwrap_or(content.len());
                let name = content[..name_end].trim().to_string();
                eprintln!("DEBUG: Tool name: {}", name);

                let mut parameters = Vec::new();
                let mut args_pos = name_end;

                while let Some(k_start) = content[args_pos..].find("<arg_key>") {
                    let k_start = args_pos + k_start + 9;
                    if let Some(k_end) = content[k_start..].find("</arg_key>") {
                        let k_end = k_start + k_end;
                        let key = content[k_start..k_end].trim().to_string();
                        eprintln!("DEBUG: Found key: {}", key);

                        let v_search_start = k_end + 10; // skip </arg_key>
                        if let Some(v_start) = content[v_search_start..].find("<arg_value>") {
                            let v_start = v_search_start + v_start + 11;
                            if let Some(v_end) = content[v_start..].find("</arg_value>") {
                                let v_end = v_start + v_end;
                                let value = content[v_start..v_end].trim().to_string();
                                eprintln!("DEBUG: Found value: {}", value);
                                parameters.push((key, value));
                                args_pos = v_end + 12; // skip </arg_value>
                                continue;
                            }
                        }
                    }
                    break;
                }

                if !name.is_empty() {
                    calls.push(XmlToolCall { name, parameters });
                }

                pos = end + 12;
            } else {
                break;
            }
        }
    }

    if calls.is_empty() {
        None
    } else {
        Some(calls)
    }
}

/// Parse Sonnet's simpler XML format
fn parse_simple_xml_format(text: &str) -> Option<String> {
    let mut messages = Vec::new();

    if text.contains("<read_file>") {
        if let Some(path) = extract_tag_content(text, "path") {
            let filename = path.split('/').last().unwrap_or(&path);
            messages.push(format!("📖 Reading `{}`...", filename));
        }
    }

    if text.contains("<write_file>") {
        if let Some(path) = extract_tag_content(text, "path") {
            let filename = path.split('/').last().unwrap_or(&path);
            messages.push(format!("✍️ Writing to `{}`...", filename));
        }
    }

    if text.contains("<edit_file>") {
        if let Some(path) = extract_tag_content(text, "path") {
            let filename = path.split('/').last().unwrap_or(&path);
            messages.push(format!("✏️ Editing `{}`...", filename));
        }
    }

    if text.contains("<list_directory>") {
        if let Some(path) = extract_tag_content(text, "path") {
            let dirname = path.split('/').last().unwrap_or(&path);
            messages.push(format!("📂 Listing `{}`...", dirname));
        }
    }

    if text.contains("<grep_search>") {
        if let Some(pattern) = extract_tag_content(text, "pattern") {
            messages.push(format!("🔍 Searching for `{}`...", pattern));
        }
    }

    if text.contains("<run_command>") {
        if let Some(cmd) = extract_tag_content(text, "command") {
            messages.push(format!("⚙️ Running `{}`...", cmd));
        }
    }

    if messages.is_empty() {
        None
    } else {
        Some(messages.join("\n"))
    }
}

fn extract_tag_content(text: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);

    let start = text.find(&start_tag)? + start_tag.len();
    let end = text.find(&end_tag)?;

    if start < end {
        Some(text[start..end].trim().to_string())
    } else {
        None
    }
}

pub fn is_xml_tool_output(text: &str) -> bool {
    // Original format
    text.contains("<function_calls>") 
    || text.contains("</function_calls>")
    || text.contains("<invoke")
    || text.contains("</invoke>")
    || text.contains("<parameter")
    || text.contains("</parameter>")
    || text.contains("<results>")
    || text.contains("</results>")
    || text.contains("<result>")
    || text.contains("</result>")
    || text.contains("<output>")
    || text.contains("</output>")
    // Sonnet's simpler format
    || text.contains("<read_file>")
    || text.contains("</read_file>")
    || text.contains("<write_file>")
    || text.contains("</write_file>")
    || text.contains("<edit_file>")
    || text.contains("</edit_file>")
    || text.contains("<list_directory>")
    || text.contains("</list_directory>")
    || text.contains("<grep_search>")
    || text.contains("</grep_search>")
    || text.contains("<run_command>")
    || text.contains("</run_command>")
    || text.contains("<path>")
    || text.contains("</path>")
    || text.contains("<pattern>")
    || text.contains("</pattern>")
    || text.contains("<command>")
    || text.contains("</command>")
    // GLM format
    || text.contains("<tool_call>")
    || text.contains("</tool_call>")
}

/// Convert XML tool calls to user-friendly status messages
pub fn xml_to_status_message(text: &str) -> Option<String> {
    if !is_xml_tool_output(text) {
        return None;
    }

    // Try Sonnet's simpler format first
    if let Some(msg) = parse_simple_xml_format(text) {
        return Some(msg);
    }

    let calls = detect_xml_tool_calls(text)?;

    let mut messages = Vec::new();
    for call in calls {
        let msg = match call.name.as_str() {
            "read_file" => {
                let path = call
                    .parameters
                    .iter()
                    .find(|(k, _)| k == "path")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("file");
                format!("📖 Reading `{}`...", path.split('/').last().unwrap_or(path))
            }
            "write_file" => {
                let path = call
                    .parameters
                    .iter()
                    .find(|(k, _)| k == "path")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("file");
                format!(
                    "✍️ Writing to `{}`...",
                    path.split('/').last().unwrap_or(path)
                )
            }
            "edit_file" => {
                let path = call
                    .parameters
                    .iter()
                    .find(|(k, _)| k == "path")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("file");
                format!("✏️ Editing `{}`...", path.split('/').last().unwrap_or(path))
            }
            "list_directory" => {
                let path = call
                    .parameters
                    .iter()
                    .find(|(k, _)| k == "path")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("directory");
                format!("📂 Listing `{}`...", path.split('/').last().unwrap_or(path))
            }
            "grep_search" => {
                let pattern = call
                    .parameters
                    .iter()
                    .find(|(k, _)| k == "pattern")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("pattern");
                format!("🔍 Searching for `{}`...", pattern)
            }
            "run_command" => {
                let cmd = call
                    .parameters
                    .iter()
                    .find(|(k, _)| k == "command")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("command");
                format!("⚙️ Running `{}`...", cmd)
            }
            _ => format!("🔧 Using tool `{}`...", call.name),
        };
        messages.push(msg);
    }

    if messages.is_empty() {
        None
    } else {
        Some(messages.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_xml_tool_calls() {
        let text = r#"<function_calls>
<invoke name="read_file">
<parameter name="path">/tmp/test.txt</parameter>
</invoke>
</function_calls>"#;

        let calls = detect_xml_tool_calls(text).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].parameters.len(), 1);
        assert_eq!(calls[0].parameters[0].0, "path");
        assert_eq!(calls[0].parameters[0].1, "/tmp/test.txt");
    }
}
