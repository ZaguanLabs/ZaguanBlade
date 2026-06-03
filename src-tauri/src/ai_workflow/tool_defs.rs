use serde_json::Value;

fn is_composite_tool(name: &str) -> bool {
    matches!(name, "read_many_files" | "batch" | "codebase_investigator")
}

fn is_reliable_composite_tool_model(model_id: &str) -> bool {
    let model = model_id.to_ascii_lowercase();
    model.contains("gpt")
        || model.contains("o3")
        || model.contains("o4")
        || model.contains("claude")
        || model.contains("codex")
        || model.contains("qwen")
        || model.contains("deepseek")
        || model.contains("gemini")
}

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
            "name": "symbol_search",
            "function": {
                "name": "symbol_search",
                "description": "Search indexed symbols by name or qualified name. Broad searches self-heal low-confidence or empty results with bounded freshness checks, targeted reindexing, exact literal fallback, and search_health metadata.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Symbol name or qualified-name query" },
                        "path": { "type": "string", "description": "Optional file path filter" },
                        "kind": { "type": "string", "description": "Optional symbol kind filter" },
                        "limit": { "type": "integer", "description": "Optional max results" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "semantic_anchor_search",
            "function": {
                "name": "semantic_anchor_search",
                "description": "Search indexed semantic anchors such as protocol tags, command names, event names, route-like strings, config keys, translation keys, and CSS/theme tokens.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Semantic literal or project concept query" },
                        "path": { "type": "string", "description": "Optional file path filter" },
                        "limit": { "type": "integer", "description": "Optional max anchors" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "symbol_resolve",
            "function": {
                "name": "symbol_resolve",
                "description": "Resolve one symbol to its exact current structural record using a stable symbol ID or file-scoped name",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "symbol_id": { "type": "string", "description": "Stable symbol ID" },
                        "path": { "type": "string", "description": "Optional file path when resolving by name" },
                        "qualified_name": { "type": "string", "description": "Optional exact qualified name" },
                        "name": { "type": "string", "description": "Optional simple symbol name" }
                    },
                    "required": [],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "symbol_outline",
            "function": {
                "name": "symbol_outline",
                "description": "Return a compact symbol inventory and optional hierarchical outline for one file using the local code-intelligence index",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path whose symbol inventory should be returned" },
                        "max_symbols": { "type": "integer", "description": "Maximum flat inventory symbols to return, capped by the backend" },
                        "limit": { "type": "integer", "description": "Alias for max_symbols" },
                        "include_outline": { "type": "boolean", "description": "Whether to include the nested hierarchy in addition to the flat inventory" }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "symbol_references",
            "function": {
                "name": "symbol_references",
                "description": "Expand inbound and outbound relationships for one symbol or important symbols in a file, including resolved-symbol confidence and name-fallback metadata",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "symbol_id": { "type": "string", "description": "Stable symbol ID for single-symbol expansion" },
                        "path": { "type": "string", "description": "File path for resolving by name, or file-wide expansion when no name is provided" },
                        "qualified_name": { "type": "string", "description": "Optional exact qualified name" },
                        "name": { "type": "string", "description": "Optional simple symbol name" },
                        "relationship": { "type": "string", "description": "Optional single relationship type: call, import, export, extends, implements, contains" },
                        "relationships": { "type": "array", "items": { "type": "string" }, "description": "Optional relationship type list" },
                        "limit": { "type": "integer", "description": "Optional max references per relationship type" },
                        "max_symbols": { "type": "integer", "description": "For file-wide expansion, maximum important symbols to expand" }
                    },
                    "required": [],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "edit_impact",
            "function": {
                "name": "edit_impact",
                "description": "Analyze likely edit impact before changing a file or symbol, including impacted files, related tests, reference counts, risk, confidence, and suggested read ranges.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Target file path, or file path for resolving by name" },
                        "symbol_id": { "type": "string", "description": "Optional stable symbol ID for single-symbol impact" },
                        "qualified_name": { "type": "string", "description": "Optional exact qualified name within path" },
                        "name": { "type": "string", "description": "Optional simple symbol name within path" },
                        "limit": { "type": "integer", "description": "Optional max impacted files and references per relationship" },
                        "max_symbols": { "type": "integer", "description": "For file-wide impact, maximum important symbols to analyze" }
                    },
                    "required": [],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "symbol_graph",
            "function": {
                "name": "symbol_graph",
                "description": "Return incoming and outgoing graph edges for one symbol using the local code-intelligence index, including call, import, export, extends, implements, and contains relationships",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "symbol_id": { "type": "string", "description": "Stable symbol ID" },
                        "path": { "type": "string", "description": "Optional file path when resolving by name" },
                        "qualified_name": { "type": "string", "description": "Optional exact qualified name" },
                        "name": { "type": "string", "description": "Optional simple symbol name" },
                        "relationship_type": { "type": "string", "description": "Optional edge kind: call, import, export, extends, implements, or contains" },
                        "limit": { "type": "integer", "description": "Optional max incoming/outgoing edges" }
                    },
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
                "description": "Apply atomic search/replace edits to an existing file",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path" },
                        "old_text": { "type": "string", "description": "Legacy single-edit mode: text to find and replace" },
                        "new_text": { "type": "string", "description": "Legacy single-edit mode: replacement text" },
                        "start_line": { "type": "integer", "description": "Optional line hint for legacy single-edit mode" },
                        "end_line": { "type": "integer", "description": "Optional end line hint for legacy single-edit mode" },
                        "patches": {
                            "type": "array",
                            "description": "Preferred multi-edit mode: array of patches applied atomically",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "old_text": { "type": "string", "description": "Exact text to replace" },
                                    "new_text": { "type": "string", "description": "Replacement text" },
                                    "start_line": { "type": "integer", "description": "Optional line hint" },
                                    "end_line": { "type": "integer", "description": "Optional end line hint" }
                                },
                                "required": ["old_text", "new_text"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["path"],
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
            "name": "get_project_index_overview",
            "function": {
                "name": "get_project_index_overview",
                "description": "Read a compact capped overview window from .zblade/context/project_index.md for first-turn orientation. Prefer this before broad repo-wide grep/search when no active file context is available.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Optional workspace root path" },
                        "max_chars": { "type": "integer", "description": "Optional output cap (default 6000, max 12000)" },
                        "offset": { "type": "integer", "description": "Optional character offset (default 0)" }
                    },
                    "required": [],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "get_project_index_chunk",
            "function": {
                "name": "get_project_index_chunk",
                "description": "Read a deterministic paged chunk from .zblade/context/project_index.md. Use this only when deeper paging is needed after get_project_index_overview.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Optional workspace root path" },
                        "offset": { "type": "integer", "description": "Optional character offset (default 0)" },
                        "max_chars": { "type": "integer", "description": "Optional output cap (default 4000, max 8000)" }
                    },
                    "required": [],
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
            "name": "read_many_files",
            "function": {
                "name": "read_many_files",
                "description": "Read many files by glob patterns in a single bounded, parallelized call. Returns per-file errors and truncation metadata.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "paths": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Glob patterns to include"
                        },
                        "exclude": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional glob patterns to exclude"
                        },
                        "max_files": {
                            "type": "integer",
                            "description": "Maximum number of files to return"
                        },
                        "max_bytes_per_file": {
                            "type": "integer",
                            "description": "Maximum bytes to read per file"
                        },
                        "include_line_numbers": {
                            "type": "boolean",
                            "description": "Include line numbers in rendered content"
                        }
                    },
                    "required": ["paths"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "batch",
            "function": {
                "name": "batch",
                "description": "Execute multiple read-only tool calls concurrently with all-settled semantics and ordered aggregation.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "calls": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "tool": { "type": "string" },
                                    "arguments": { "type": "object" }
                                },
                                "required": ["tool", "arguments"],
                                "additionalProperties": false
                            }
                        },
                        "max_parallel": {
                            "type": "integer",
                            "description": "Maximum concurrent calls"
                        },
                        "fail_fast": {
                            "type": "boolean",
                            "description": "Stop remaining queued calls after first failure"
                        },
                        "ordered": {
                            "type": "boolean",
                            "description": "Preserve input order in results"
                        },
                        "cancel_after_ms": {
                            "type": "integer",
                            "description": "Optional time budget in milliseconds before cancelling queued calls"
                        }
                    },
                    "required": ["calls"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "codebase_investigator",
            "function": {
                "name": "codebase_investigator",
                "description": "Run a bounded read-only investigation pass and return structured findings with evidence references.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "objective": {
                            "type": "string",
                            "description": "Investigation goal"
                        },
                        "scope": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional glob scope for files"
                        },
                        "max_turns": {
                            "type": "integer",
                            "description": "Upper bound for investigation turns"
                        },
                        "max_tool_calls": {
                            "type": "integer",
                            "description": "Upper bound for internal read calls"
                        },
                        "output_format": {
                            "type": "string",
                            "description": "Output format: json (default) or markdown"
                        },
                        "cancel_after_ms": {
                            "type": "integer",
                            "description": "Optional time budget in milliseconds"
                        }
                    },
                    "required": ["objective"],
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
                "description": "Execute command (requires approval). Prefer structured form with program+args. Use command only when shell behavior is required.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Legacy shell command line" },
                        "program": { "type": "string", "description": "Executable name/path for structured non-shell execution" },
                        "args": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Arguments for program in structured non-shell mode"
                        },
                        "shell": { "type": "boolean", "description": "Force shell execution. Defaults to false when program is used, true for legacy command" },
                        "cwd": { "type": "string", "description": "Working directory" }
                    },
                    "required": [],
                    "additionalProperties": false
                }
            }
        }),
        // Note: todo_write is server-side only (handled by zcoderd)
    ]
}

pub fn get_tool_definitions_for_model(model_id: &str, composite_tools_enabled: bool) -> Vec<Value> {
    let include_composite = composite_tools_enabled && is_reliable_composite_tool_model(model_id);
    if include_composite {
        return get_tool_definitions();
    }

    get_tool_definitions()
        .into_iter()
        .filter(|def| {
            !def.get("name")
                .and_then(|v| v.as_str())
                .map(is_composite_tool)
                .unwrap_or(false)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{get_tool_definitions, get_tool_definitions_for_model};

    #[test]
    fn includes_composite_tool_definitions() {
        let defs = get_tool_definitions();
        let names = defs
            .iter()
            .filter_map(|value| value.get("name").and_then(|v| v.as_str()))
            .collect::<Vec<&str>>();

        assert!(names.contains(&"read_many_files"));
        assert!(names.contains(&"batch"));
        assert!(names.contains(&"codebase_investigator"));
    }

    #[test]
    fn excludes_composite_tools_for_unqualified_model() {
        let defs = get_tool_definitions_for_model("tiny-random-model", true);
        let names = defs
            .iter()
            .filter_map(|value| value.get("name").and_then(|v| v.as_str()))
            .collect::<Vec<&str>>();

        assert!(!names.contains(&"read_many_files"));
        assert!(!names.contains(&"batch"));
        assert!(!names.contains(&"codebase_investigator"));
    }

    #[test]
    fn includes_composite_tools_for_reliable_model_when_enabled() {
        let defs = get_tool_definitions_for_model("gpt-5.2", true);
        let names = defs
            .iter()
            .filter_map(|value| value.get("name").and_then(|v| v.as_str()))
            .collect::<Vec<&str>>();

        assert!(names.contains(&"read_many_files"));
        assert!(names.contains(&"batch"));
        assert!(names.contains(&"codebase_investigator"));
    }

    #[test]
    fn excludes_composite_tools_when_flag_disabled() {
        let defs = get_tool_definitions_for_model("gpt-5.2", false);
        let names = defs
            .iter()
            .filter_map(|value| value.get("name").and_then(|v| v.as_str()))
            .collect::<Vec<&str>>();

        assert!(!names.contains(&"read_many_files"));
        assert!(!names.contains(&"batch"));
        assert!(!names.contains(&"codebase_investigator"));
    }
}
