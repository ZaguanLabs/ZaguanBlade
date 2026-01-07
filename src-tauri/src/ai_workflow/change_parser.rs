use serde_json::Value;
use std::fs;
use std::path::Path;

use super::{PendingChange, ChangeType};

pub fn parse_change_args(raw_args: &str, workspace_root: &Path, tool_name: &str) -> Result<PendingChange, String> {
    let v: Value =
        serde_json::from_str(raw_args).map_err(|e| format!("invalid tool args json: {}", e))?;

    let obj = v
        .as_object()
        .ok_or_else(|| "invalid args: expected object".to_string())?;

    // Get path
    let path = obj
        .get("path")
        .or_else(|| obj.get("file_path"))
        .or_else(|| obj.get("filepath"))
        .or_else(|| obj.get("filename"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing required arg: path".to_string())?
        .to_string();

    // Validate path is under workspace
    let ws = fs::canonicalize(workspace_root).map_err(|e| e.to_string())?;
    let requested = Path::new(&path);
    let target = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        ws.join(requested)
    };

    // For new files, the file itself might not exist, so we can't canonicalize it directly.
    // Let's try to canonicalize the PARENT directory if possible to ensure it's in workspace
    if let Some(parent) = target.parent() {
        if parent.exists() {
            if let Ok(canon_parent) = fs::canonicalize(parent) {
                if !canon_parent.starts_with(&ws) {
                    return Err("path (parent) is outside workspace".to_string());
                }
            }
        }
    }

    // Determine change type based on tool name and file existence
    let change_type = match tool_name {
        "delete_file" => ChangeType::DeleteFile,
        "write_file" | "create_file" => {
            // Always a new file for these tools
            let content = obj
                .get("content")
                .or_else(|| obj.get("contents"))
                .or_else(|| obj.get("text"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing required arg: content".to_string())?
                .to_string();
            ChangeType::NewFile { content }
        }
        "edit_file" | "apply_edit" | "apply_patch" => {
            // Patch operation
            let old_content = obj
                .get("old_content")
                .or_else(|| obj.get("old"))
                .or_else(|| obj.get("from"))
                .or_else(|| obj.get("old_text"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    // Try to read existing file content
                    fs::read_to_string(&target).unwrap_or_default()
                });

            let new_content = obj
                .get("new_content")
                .or_else(|| obj.get("new"))
                .or_else(|| obj.get("to"))
                .or_else(|| obj.get("new_text"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing required arg: new_content".to_string())?
                .to_string();

            ChangeType::Patch { old_content, new_content }
        }
        _ => return Err(format!("unsupported tool for change parsing: {}", tool_name)),
    };

    Ok(PendingChange {
        call: crate::protocol::ToolCall {
            id: String::new(), // Will be filled by caller
            typ: "function".to_string(),
            function: crate::protocol::ToolFunction {
                name: String::new(),
                arguments: String::new(),
            },
            status: Some("executing".to_string()),
            result: None,
        },
        path,
        change_type,
    })
}
