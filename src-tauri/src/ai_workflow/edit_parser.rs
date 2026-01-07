use serde_json::Value;
use std::fs;
use std::path::Path;

use super::PendingEdit;

pub fn parse_edit_args(raw_args: &str, workspace_root: &Path) -> Result<PendingEdit, String> {
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
    // Instead, we check the parent or just resolve components carefully.
    // Ideally, we want to ensure target is strictly under ws.

    // Simple check: does it look like it's trying to escape?
    // Using simple path resolution (without fs access for the file itself)
    // Note: This is a simplified check. For strict security, we'd want to normalize .. components.
    // But since we are intercepting for USER REVIEW, strict sandbox enforcement here is less critical
    // than in the actual tool execution (which does its own validation).
    // The most important thing is that the UI displays a path that looks correct.

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

    // Get old/new content
    // For write_file, old_content is usually not provided.
    // If it's missing, try to read the current file content from disk to provide a proper diff.
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
        .or_else(|| obj.get("content")) // Handle write_file 'content' arg
        .or_else(|| obj.get("contents"))
        .or_else(|| obj.get("text"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing required arg: new_content (or content/text)".to_string())?
        .to_string();

    Ok(PendingEdit {
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
        old_content,
        new_content,
        is_new_file: false, // Will be set by caller based on tool name and content
    })
}
