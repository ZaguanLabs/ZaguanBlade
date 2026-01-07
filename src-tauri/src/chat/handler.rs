use crate::protocol::{ToolCall, ToolCallDelta, ToolFunction};

pub fn merge_tool_call_deltas(buf: &mut Vec<ToolCall>, deltas: &[ToolCallDelta]) {
    for d in deltas {
        // DEBUG: Log what we're receiving
        eprintln!(
            "[DELTA] index={}, id={:?}, type={:?}, function.name={:?}, function.args={:?}",
            d.index,
            d.id,
            d.typ,
            d.function.as_ref().and_then(|f| f.name.as_ref()),
            d.function.as_ref().and_then(|f| f.arguments.as_ref())
        );

        while buf.len() <= d.index {
            // Generate a fallback ID for models that don't provide one
            let fallback_id = format!("tool_call_{}", d.index);
            buf.push(ToolCall {
                id: fallback_id,
                typ: "function".to_string(),
                function: ToolFunction {
                    name: String::new(),
                    arguments: String::new(),
                },
                status: Some("executing".to_string()),
                result: None,
            });
        }

        let entry = &mut buf[d.index];
        if let Some(id) = &d.id {
            if !id.is_empty() {
                entry.id = id.clone();
            }
        }
        if let Some(t) = &d.typ {
            if !t.is_empty() {
                entry.typ = t.clone();
            }
        }
        if let Some(f) = &d.function {
            // CRITICAL: For Qwen models, name and arguments may arrive separately
            // We need to accumulate both, not just append arguments
            if let Some(name) = &f.name {
                if !name.is_empty() {
                    // If we have a name, set it (Qwen sends this in first chunk)
                    entry.function.name = name.clone();
                    eprintln!("[DELTA MERGE] Set function name: {}", name);
                }
            }
            if let Some(args) = &f.arguments {
                // Accumulate arguments (may come in multiple chunks)
                entry.function.arguments.push_str(args);
                eprintln!(
                    "[DELTA MERGE] Accumulated args, total length: {}",
                    entry.function.arguments.len()
                );
            }
        }
    }
}

pub fn extract_tool_path_arg(raw_args: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw_args).ok()?;
    let obj = v.as_object()?;
    obj.get("path")
        .or_else(|| obj.get("file_path"))
        .or_else(|| obj.get("filepath"))
        .or_else(|| obj.get("filename"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}
