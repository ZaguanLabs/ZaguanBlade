use serde_json::Value;

/// Returns the full list of tools available to the AI model.
/// These are defined in OpenAI's JSON Schema format for tool calling.
pub fn get_tool_definitions() -> Vec<Value> {
    vec![
        // 1. Get Editor State
        serde_json::json!({
            "type": "function",
            "name": "get_editor_state", // Top-level name required by some endpoints
            "function": {
                "name": "get_editor_state",
                "description": "Get current workspace context including active file path, cursor position, and open files.

WORKFLOW - When user says 'this file', 'here', or 'current file':
1. Call get_editor_state ONCE to get active_file path
2. Extract the active_file field from the result
3. Call read_file with that exact path
4. NEVER call get_editor_state again - you already have the path

WARNING: Do NOT loop calling get_editor_state. It returns the same active_file path every time. Repeating this call indicates you are stuck in a loop.

EXAMPLE:
User: 'Find all the structs in this file'
→ get_editor_state (returns active_file: '/workspace/src/main.rs')
→ read_file with path='/workspace/src/main.rs'
→ Analyze and list the structs",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {},
                    "required": [],
                    "additionalProperties": false
                }
            }
        }),
        // 2. Read File Range
        serde_json::json!({
            "type": "function",
            "name": "read_file_range",
            "function": {
                "name": "read_file_range",
                "description": "Read specific line ranges from a file (1-indexed line numbers).

WHEN TO USE:
- You know exact line numbers and only need a small section
- Viewing specific functions or code blocks
- User explicitly requests specific lines (e.g., 'Show me lines 50-70')

WHEN NOT TO USE:
- Exploring unfamiliar code (use read_file instead)
- Need to understand full file context
- Don't know exact line numbers

FEATURE: When called, this automatically opens the file in the editor with the specified lines highlighted and scrolled into view.

EXAMPLE:
User: 'Show me lines 50-70 of main.rs'
→ read_file_range(path='/workspace/src/main.rs', start_line=50, end_line=70, context_lines=3)
→ File opens with lines 50-70 highlighted in yellow",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file (relative to workspace root or absolute)"
                        },
                        "start_line": {
                            "type": "integer",
                            "description": "Starting line number (1-indexed, defaults to 1)"
                        },
                        "end_line": {
                            "type": "integer",
                            "description": "Ending line number (1-indexed, defaults to end of file)"
                        },
                        "context_lines": {
                            "type": "integer",
                            "description": "Number of additional lines to include before and after the range for context (defaults to 0)"
                        }
                    },
                    "required": ["path", "start_line", "end_line", "context_lines"],
                    "additionalProperties": false
                }
            }
        }),
        // 3. Apply Patch (renamed from apply_edit)
        serde_json::json!({
            "type": "function",
            "name": "apply_patch",
            "function": {
                "name": "apply_patch",
                "description": "Edit existing file using exact SEARCH/REPLACE.

CRITICAL WORKFLOW:
1. ALWAYS read_file first to see exact text
2. Copy exact text including whitespace for old_text
3. Provide new_text with your changes
4. ONE patch per file (combine multiple changes into one patch)

CRITICAL RULES:
- old_text must match byte-for-byte including all whitespace
- Multiple patches for same file = WRONG (combine them)
- Read the file first - never guess the exact text

APPROVAL FLOW:
- This triggers a diff review UI
- User will see the proposed changes
- User must Accept or Reject
- Only applies if user accepts

EXAMPLE:
User: 'Change max_size default from 100 to 200'
→ grep_search for 'max_size' (find it in utils.rs:14)
→ read_file('utils.rs') (see exact text: 'max_size: 100,')
→ apply_patch(path='utils.rs', old_text='max_size: 100,', new_text='max_size: 200,')
→ User sees diff and clicks Accept/Reject

COMMON MISTAKES:
- Not reading file first
- Whitespace mismatch (tabs vs spaces)
- Multiple patches for same file
- Guessing the exact text",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to edit"
                        },
                        "old_text": {
                            "type": "string",
                            "description": "The exact text to replace (must match exactly including whitespace)"
                        },
                        "new_text": {
                            "type": "string",
                            "description": "The new text to insert in place of old_text"
                        }
                    },
                    "required": ["path", "old_text", "new_text"],
                    "additionalProperties": false
                }
            }
        }),
        // 4. Get Workspace Structure
        serde_json::json!({
            "type": "function",
            "name": "get_workspace_structure",
            "function": {
                "name": "get_workspace_structure",
                "description": "Get project directory tree with file sizes and types.

WHEN TO USE:
- First time exploring an unfamiliar codebase
- Understanding project organization
- Finding where different components live
- More efficient than list_dir for project overview

RETURNS:
- Directory tree structure
- File sizes
- File types (file/directory)
- Relative paths from workspace root

EXAMPLE:
User: 'What's the structure of this project?'
→ get_workspace_structure(path='.')
→ Shows full tree with src/, tests/, Cargo.toml, etc.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to start from (defaults to workspace root)"
                        },
                        "max_depth": {
                            "type": "integer",
                            "description": "Maximum depth to traverse (defaults to 3)"
                        },
                        "include_hidden": {
                            "type": "boolean",
                            "description": "Whether to include hidden files/directories (defaults to false)"
                        }
                    },
                    "required": ["path", "max_depth", "include_hidden"],
                    "additionalProperties": false
                }
            }
        }),
        // 5. Read File
        serde_json::json!({
            "type": "function",
            "name": "read_file",
            "function": {
                "name": "read_file",
                "description": "Read complete file contents. WORKFLOW: (1) Check workspace context, (2) Read file, (3) Edit. Always read before editing to get exact content for matching.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to read"
                        }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }
            }
        }),
        // 6. Write File
        serde_json::json!({
            "type": "function",
            "name": "write_file",
            "function": {
                "name": "write_file",
                "description": "Create NEW file or overwrite existing. Use ONLY for: (1) new files, (2) complete rewrites. For edits, use apply_patch.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to the file to write"
                        },
                        "content": {
                            "type": "string",
                            "description": "The complete content to write to the file"
                        }
                    },
                    "required": ["path", "content"],
                    "additionalProperties": false
                }
            }
        }),
        // 7. RG (renamed from grep_search)
        serde_json::json!({
            "type": "function",
            "name": "rg",
            "function": {
                "name": "rg",
                "description": "Fast exact string search across codebase. Use BEFORE reading files to locate code. Find: function definitions, imports, constants, error messages.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Regex pattern to search for"
                        },
                        "path": {
                            "type": "string",
                            "description": "Path to search in (defaults to workspace root)"
                        }
                    },
                    "required": ["pattern", "path"],
                    "additionalProperties": false
                }
            }
        }),
        // 8. List Dir (renamed from list_directory)
        serde_json::json!({
            "type": "function",
            "name": "list_dir",
            "function": {
                "name": "list_dir",
                "description": "List directory contents. For project overview, use get_workspace_structure instead.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Path to list (defaults to workspace root)"
                        }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }
            }
        }),
        // 9. Run Command
        serde_json::json!({
            "type": "function",
            "name": "run_command",
            "function": {
                "name": "run_command",
                "description": "Execute shell command. Requires user approval. Always specify cwd. Use for: tests, dependencies, servers.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute"
                        },
                        "cwd": {
                            "type": "string",
                            "description": "Working directory for the command (defaults to workspace root)"
                        }
                    },
                    "required": ["command", "cwd"],
                    "additionalProperties": false
                }
            }
        }),
        // Note: todo_write is now server-side only (handled by zcoderd)
    ]
}
