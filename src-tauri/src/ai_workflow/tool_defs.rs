use serde_json::Value;

/// Tool definitions for zblade's internal tool execution.
///
/// NOTE: These are NOT prompts for the AI model - prompting is zcoderd's responsibility.
/// These schemas define how zblade parses and executes tool calls received from zcoderd.
pub fn get_tool_definitions() -> Vec<Value> {
    vec![
        serde_json::json!({
            "type": "function",
            "name": "get_editor_state",
            "function": {
                "name": "get_editor_state",
                "description": "Get current editor context (active file, cursor position, open files)",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {},
                    "required": [],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "read_file_range",
            "function": {
                "name": "read_file_range",
                "description": "Read specific line range from a file",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path" },
                        "start_line": { "type": "integer", "description": "Start line (1-indexed)" },
                        "end_line": { "type": "integer", "description": "End line (1-indexed)" },
                        "context_lines": { "type": "integer", "description": "Extra context lines" }
                    },
                    "required": ["path", "start_line", "end_line", "context_lines"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "apply_patch",
            "function": {
                "name": "apply_patch",
                "description": "Apply search/replace edit to a file",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path" },
                        "old_text": { "type": "string", "description": "Text to find and replace" },
                        "new_text": { "type": "string", "description": "Replacement text" }
                    },
                    "required": ["path", "old_text", "new_text"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "get_workspace_structure",
            "function": {
                "name": "get_workspace_structure",
                "description": "Get directory tree structure",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Starting path" },
                        "max_depth": { "type": "integer", "description": "Max traversal depth" },
                        "include_hidden": { "type": "boolean", "description": "Include hidden files" }
                    },
                    "required": ["path", "max_depth", "include_hidden"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "read_file",
            "function": {
                "name": "read_file",
                "description": "Read complete file contents",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path" }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "write_file",
            "function": {
                "name": "write_file",
                "description": "Write content to file",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path" },
                        "content": { "type": "string", "description": "File content" }
                    },
                    "required": ["path", "content"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "rg",
            "function": {
                "name": "rg",
                "description": "Search files with ripgrep",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Search pattern" },
                        "path": { "type": "string", "description": "Search path" }
                    },
                    "required": ["pattern", "path"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "list_dir",
            "function": {
                "name": "list_dir",
                "description": "List directory contents",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Directory path" }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "run_command",
            "function": {
                "name": "run_command",
                "description": "Execute shell command (requires approval)",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Shell command" },
                        "cwd": { "type": "string", "description": "Working directory" }
                    },
                    "required": ["command", "cwd"],
                    "additionalProperties": false
                }
            }
        }),
        // Note: todo_write is server-side only (handled by zcoderd)
    ]
}
