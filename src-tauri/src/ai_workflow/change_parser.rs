use serde_json::Value;
use std::fs;
use std::path::Path;

use super::{ChangeType, PatchHunk, PendingChange};

pub fn parse_change_args(
    raw_args: &str,
    workspace_root: &Path,
    tool_name: &str,
) -> Result<PendingChange, String> {
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
        .or_else(|| obj.get("TargetFile"))
        .or_else(|| obj.get("target_file"))
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
        "delete_file" => ChangeType::DeleteFile { old_content: None },
        "write_file" | "write_file_validated" | "create_file" | "write_to_file" => {
            // Always a new file for these tools
            let content = obj
                .get("content")
                .or_else(|| obj.get("contents"))
                .or_else(|| obj.get("text"))
                .or_else(|| obj.get("data"))
                .or_else(|| obj.get("CodeContent"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing required arg: content".to_string())?
                .to_string();
            ChangeType::NewFile { content }
        }
        "edit_file"
        | "apply_edit"
        | "apply_patch"
        | "apply_patch_validated"
        | "replace_file_content"
        | "multi_replace_file_content" => {
            // Check for new multi-patch format FIRST
            if let Some(patches_value) = obj.get("patches") {
                if let Some(patches_arr) = patches_value.as_array() {
                    let patches: Vec<PatchHunk> = patches_arr
                        .iter()
                        .filter_map(|p| {
                            let patch_obj = p.as_object()?;
                            Some(PatchHunk {
                                old_text: patch_obj.get("old_text")?.as_str()?.to_string(),
                                new_text: patch_obj.get("new_text")?.as_str()?.to_string(),
                                start_line: patch_obj
                                    .get("start_line")
                                    .and_then(|v| v.as_u64())
                                    .map(|n| n as usize),
                                end_line: patch_obj
                                    .get("end_line")
                                    .and_then(|v| v.as_u64())
                                    .map(|n| n as usize),
                            })
                        })
                        .collect();

                    if !patches.is_empty() {
                        return Ok(PendingChange {
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
                            change_type: ChangeType::MultiPatch { patches },
                            applied: false,
                            error: None,
                        });
                    }
                }
            }

            // Fall back to legacy single-patch format
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
                .ok_or_else(|| "missing required arg: new_content (or patches array)".to_string())?
                .to_string();

            ChangeType::Patch {
                old_content,
                new_content,
            }
        }
        _ => {
            return Err(format!(
                "unsupported tool for change parsing: {}",
                tool_name
            ))
        }
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
        applied: false,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_change_args;
    use crate::ai_workflow::ChangeType;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn parse_change_args_supports_write_to_file_aliases() {
        let workspace = tempdir().expect("tempdir");
        let raw_args = json!({
            "TargetFile": "nested/example.txt",
            "CodeContent": "hello"
        })
        .to_string();

        let change = parse_change_args(&raw_args, workspace.path(), "write_to_file")
            .expect("write_to_file alias should parse");

        assert_eq!(change.path, "nested/example.txt");
        match change.change_type {
            ChangeType::NewFile { content } => assert_eq!(content, "hello"),
            _ => panic!("expected NewFile change type"),
        }
    }

    #[test]
    fn parse_change_args_supports_replace_file_content_aliases() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("example.txt");
        std::fs::write(&file_path, "before").expect("seed file");

        let raw_args = json!({
            "path": "example.txt",
            "old_content": "before",
            "new_content": "after"
        })
        .to_string();

        let change = parse_change_args(&raw_args, workspace.path(), "replace_file_content")
            .expect("replace_file_content alias should parse");

        assert_eq!(change.path, "example.txt");
        match change.change_type {
            ChangeType::Patch {
                old_content,
                new_content,
            } => {
                assert_eq!(old_content, "before");
                assert_eq!(new_content, "after");
            }
            _ => panic!("expected Patch change type"),
        }
    }

    #[test]
    fn parse_change_args_supports_multi_replace_file_content_alias() {
        let workspace = tempdir().expect("tempdir");
        let raw_args = json!({
            "path": "example.txt",
            "patches": [
                {
                    "old_text": "alpha",
                    "new_text": "beta"
                }
            ]
        })
        .to_string();

        let change = parse_change_args(&raw_args, workspace.path(), "multi_replace_file_content")
            .expect("multi_replace_file_content alias should parse");

        match change.change_type {
            ChangeType::MultiPatch { patches } => {
                assert_eq!(patches.len(), 1);
                assert_eq!(patches[0].old_text, "alpha");
                assert_eq!(patches[0].new_text, "beta");
            }
            _ => panic!("expected MultiPatch change type"),
        }
    }

    #[test]
    fn parse_change_args_supports_apply_patch_validated_alias() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("example.txt");
        std::fs::write(&file_path, "before").expect("seed file");

        let raw_args = json!({
            "path": "example.txt",
            "old_text": "before",
            "new_text": "after"
        })
        .to_string();

        let change = parse_change_args(&raw_args, workspace.path(), "apply_patch_validated")
            .expect("apply_patch_validated alias should parse");

        match change.change_type {
            ChangeType::Patch {
                old_content,
                new_content,
            } => {
                assert_eq!(old_content, "before");
                assert_eq!(new_content, "after");
            }
            _ => panic!("expected Patch change type"),
        }
    }
}
