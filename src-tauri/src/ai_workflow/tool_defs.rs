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
/// NOTE: For locally hosted models these schemas ARE model-facing — names, parameters,
/// and descriptions are sent to the model verbatim, so descriptions must be self-contained
/// teaching text (when to use the tool, how to chain results). For remote models, zcoderd
/// owns a mirrored copy of these schemas; contract changes must be handed off via a spec in
/// docs/internal so the two stay in sync. zblade also uses these schemas to parse and
/// execute tool calls regardless of model host.
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
            "name": "fast_context",
            "function": {
                "name": "fast_context",
                "description": "Plan broad or uncertain code tasks before reading many files. Returns targeted symbol-aware context: structured project context, a bounded confidence-filtered graph neighborhood, ranked primary files, enriched symbol and semantic-anchor metadata, language-support capability metadata, compact index schema summary, indexed Markdown sections, related files, optional impact hints, index health, confidence, suggested read ranges, and next steps. Legacy project-index Markdown is excluded unless explicitly requested for fallback compatibility.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "User task or investigation query" },
                        "queries": { "type": "array", "items": { "type": "string" }, "description": "Optional additional search queries" },
                        "intent": { "type": "string", "description": "Optional task intent such as bug_fix, feature, refactor, or docs" },
                        "detail": { "type": "string", "enum": ["compact", "expanded"], "description": "Response projection. Defaults to compact." },
                        "max_results": { "type": "integer", "description": "Optional max primary files" },
                        "include_tests": { "type": "boolean", "description": "Whether to include likely tests" },
                        "include_docs": { "type": "boolean", "description": "Whether to include related docs" },
                        "include_memory": { "type": "boolean", "description": "Whether to include local project memories" },
                        "include_graph_context": { "type": "boolean", "description": "Whether to include a bounded connected symbol subgraph; defaults to true" },
                        "include_project_index_min": { "type": "boolean", "description": "Legacy fallback only. When true, include project_index_min Markdown if available. Defaults to false." }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "symbol_search",
            "function": {
                "name": "symbol_search",
                "description": "First choice for structural questions when the location is unknown — use before grep, broad text search, or whole-file reads to find definitions, functions, types, and other named symbols. Searches indexed symbols by name or qualified name. Supports exact path, file_pattern, name_pattern, qualified_name_pattern, symbol kind, pagination, and compact opt-in connected-symbol previews. Returns search_health, index_health, and language_support metadata. Copy the returned symbol id exactly for follow-up tools; never construct an id from a path and name. Empty results are trustworthy only when the target file type is supported and the index is fresh; for unsupported files, literal/theme tokens, routes, config keys, translation keys, or arbitrary text use semantic_anchor_search, grep_search, or codebase_search.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Symbol name or qualified-name query" },
                        "path": { "type": "string", "description": "Optional file path filter" },
                        "file_pattern": { "type": "string", "description": "Optional comma-separated file path glob or substring filter, for example src/**/*.css" },
                        "name_pattern": { "type": "string", "description": "Optional comma-separated symbol-name glob or substring filter" },
                        "qualified_name_pattern": { "type": "string", "description": "Optional comma-separated qualified-name glob or substring filter" },
                        "kind": { "type": "string", "description": "Optional symbol kind filter" },
                        "limit": { "type": "integer", "description": "Optional max results" },
                        "offset": { "type": "integer", "description": "Optional zero-based result offset for pagination. Use only after checking has_more in a prior response." },
                        "include_connected": { "type": "boolean", "description": "Optional compact one-hop connected-symbol preview for returned results; use symbol_references for one-hop expansion or symbol_trace for bounded multi-hop traversal." },
                        "explain": { "type": "boolean", "description": "Include deterministic matched-token, field, coverage, phrase, and confidence score components." }
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
                "description": "Search indexed semantic anchors such as rationale comments, JSX section labels, design-document/code references, Markdown links, protocol tags, command/event names, routes, config keys, translations, and CSS/theme tokens. Use for non-structural evidence — section labels, rationale comments, translation keys, routes — that symbol_search does not model. Optional mode: \"phrase\" (default) matches the exact contiguous phrase; \"all_terms\" requires every normalized term to match; \"any_terms\" does ranked discovery and reports per-anchor matched_terms/total_terms coverage. Contextual anchors expose their nearest owner symbol and deterministic target path/symbol when resolvable; ambiguous targets remain unresolved. Returns language_support metadata for the optional file filter.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Semantic literal or project concept query" },
                        "path": { "type": "string", "description": "Optional file path filter" },
                        "limit": { "type": "integer", "description": "Optional max anchors" },
                        "mode": { "type": "string", "enum": ["phrase", "all_terms", "any_terms"], "description": "Anchor matching mode: phrase (default, exact contiguous phrase), all_terms (every normalized term must match), or any_terms (ranked discovery with matched_terms/total_terms coverage)" }
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
                "description": "Resolve one symbol to its exact current structural record using a stable symbol ID or file-scoped name. Returns language_support metadata for the resolved symbol file. A symbol_id must be copied exactly from a prior result; invented ids are rejected with an error.",
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
            "name": "symbol_related",
            "function": {
                "name": "symbol_related",
                "description": "Return evidence-backed symbols related to a seed symbol, including direct graph edges, same-module exports, module importers, consumers of sibling exports, and bounded lexical-similarity fallbacks. Lexical matches are labeled as heuristic evidence, not structural graph truth. Returns language_support metadata for the seed file. A symbol_id must be copied exactly from a prior result; invented ids are rejected with an error.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "symbol_id": { "type": "string", "description": "Stable symbol ID for related-symbol expansion" },
                        "path": { "type": "string", "description": "File path when resolving by name" },
                        "qualified_name": { "type": "string", "description": "Optional exact qualified name" },
                        "name": { "type": "string", "description": "Optional simple symbol name" },
                        "limit": { "type": "integer", "description": "Optional max related symbols" }
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
                "description": "Use when the file is known but the relevant range is not — call this before reading the whole file, then read only the returned ranges. Returns a compact symbol inventory and optional hierarchical outline for one file using the local code-intelligence index. Returns language_support diagnostics so unsupported or partial file types can be handled with read_file_range, semantic_anchor_search, grep_search, or codebase_search.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path whose symbol inventory should be returned" },
                        "max_symbols": { "type": "integer", "description": "Maximum declarations to return; defaults to 20" },
                        "limit": { "type": "integer", "description": "Alias for max_symbols" },
                        "nested": { "type": "boolean", "description": "Include nested members up to max_outline_depth. Defaults to false." },
                        "include_outline": { "type": "boolean", "description": "Compatibility alias for nested." },
                        "include_imports": { "type": "boolean", "description": "Include import declarations. Defaults to false." },
                        "include_locals": { "type": "boolean", "description": "Include declarations inside functions or methods. Defaults to false." },
                        "max_outline_nodes": { "type": "integer", "description": "Maximum compact outline nodes to return when include_outline is true, capped by the backend" },
                        "max_outline_depth": { "type": "integer", "description": "Maximum compact outline nesting depth when include_outline is true, capped by the backend" },
                        "include_docstrings": { "type": "boolean", "description": "Whether to include truncated docstring previews in inventory entries. Defaults to false." }
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
                "description": "Answer where-used questions with indexed graph edges. Callers of a function: incoming call edges. Implementers of an interface/trait: incoming implements edges. Subclasses: incoming extends edges. Consumers of a constant or value: incoming usage edges. Pass a symbol_id copied exactly from a prior symbol_search/symbol_outline result; never construct an id from a path and name — invented ids are rejected with an error. Expands inbound and outbound relationships for one symbol or important symbols in a file, including resolved-symbol confidence, name-fallback metadata, and language_support metadata. Low-signal generic unresolved edges (len, clone, push, new, get, insert, map, ...) are suppressed by default and counted in low_signal_suppressed; set include_low_signal=true for the exhaustive view.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "symbol_id": { "type": "string", "description": "Stable symbol ID for single-symbol expansion" },
                        "path": { "type": "string", "description": "File path for resolving by name, or file-wide expansion when no name is provided" },
                        "query": { "type": "string", "description": "Exact name or qualified name; use with path when no symbol_id is available" },
                        "qualified_name": { "type": "string", "description": "Optional exact qualified name" },
                        "name": { "type": "string", "description": "Optional simple symbol name" },
                        "relationship": { "type": "string", "description": "Optional single relationship type: call, import, export, extends, implements, contains, usage, uses_type, reads_env, or handles" },
                        "relationships": { "type": "array", "items": { "type": "string" }, "description": "Optional relationship type list" },
                        "relationship_kinds": { "type": "array", "items": { "type": "string" }, "description": "Alias for relationships" },
                        "direction": { "type": "string", "enum": ["incoming", "outgoing", "both"], "description": "Edge direction; defaults to both" },
                        "limit": { "type": "integer", "description": "Optional max references per relationship type" },
                        "max_symbols": { "type": "integer", "description": "For file-wide expansion, maximum important symbols to expand" },
                        "include_low_signal": { "type": "boolean", "description": "Include low-signal generic unresolved edges that are suppressed by default and reported via low_signal_suppressed. Defaults to false." },
                        "offset": { "type": "integer", "description": "Skip this many ranked edges per relationship group before returning results. Use only after a truncated result; pages index the ranked post-suppression list and are stable only while the index is unchanged." },
                        "cursor": { "type": "string", "description": "Continuation cursor returned by a previous symbol_references response" }
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
                "description": "Analyze likely edit impact before changing a file or symbol. Performs bounded transitive incoming traversal plus direct outgoing dependency analysis, returning evidence paths, impacted files, related tests, risk, confidence, suggested read ranges, and language_support metadata. A symbol_id must be copied exactly from a prior result; invented ids are rejected with an error. Low-signal generic unresolved edges are suppressed by default and reported via low_signal_suppressed; set include_low_signal=true for the exhaustive view.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Target file path, or file path for resolving by name" },
                        "symbol_id": { "type": "string", "description": "Optional stable symbol ID for single-symbol impact" },
                        "qualified_name": { "type": "string", "description": "Optional exact qualified name within path" },
                        "name": { "type": "string", "description": "Optional simple symbol name within path" },
                        "limit": { "type": "integer", "description": "Optional max impacted files and references per relationship" },
                        "max_symbols": { "type": "integer", "description": "For file-wide impact, maximum important symbols to analyze" },
                        "depth": { "type": "integer", "description": "Incoming impact traversal depth; defaults to 2 and is capped at 4" },
                        "edge_limit": { "type": "integer", "description": "Total incoming traversal edge budget across target symbols; defaults to 160 and is capped at 400" },
                        "per_node_limit": { "type": "integer", "description": "Per-node per-relationship expansion cap; defaults to 16 and is capped at 50" },
                        "include_low_signal": { "type": "boolean", "description": "Include low-signal generic unresolved edges that are suppressed by default and reported via low_signal_suppressed. Defaults to false." }
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
                "description": "Return incoming and outgoing graph edges for one symbol using the local code-intelligence index, including call, import, export, extends, implements, contains, usage, type-use, environment-read, and route-handler relationships. Returns language_support metadata for the seed file. A symbol_id must be copied exactly from a prior result; invented ids are rejected with an error. Low-signal generic unresolved edges are suppressed by default and reported via low_signal_suppressed; set include_low_signal=true for the exhaustive view.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "symbol_id": { "type": "string", "description": "Stable symbol ID" },
                        "path": { "type": "string", "description": "Optional file path when resolving by name" },
                        "qualified_name": { "type": "string", "description": "Optional exact qualified name" },
                        "name": { "type": "string", "description": "Optional simple symbol name" },
                        "relationship_type": { "type": "string", "description": "Optional edge kind: call, import, export, extends, implements, contains, usage, uses_type, reads_env, or handles" },
                        "limit": { "type": "integer", "description": "Optional max incoming/outgoing edges" },
                        "include_low_signal": { "type": "boolean", "description": "Include low-signal generic unresolved edges that are suppressed by default and reported via low_signal_suppressed. Defaults to false." }
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
                "description": "Apply atomic search/replace edits to an existing file. Each patch must change the file: old_text must be non-empty, copied exactly from the file, and differ from new_text. Use write_file to create or fully rewrite a file.",
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
                                    "old_text": { "type": "string", "description": "Exact text to replace; must be non-empty and differ from new_text" },
                                    "new_text": { "type": "string", "description": "Replacement text; empty string deletes old_text" },
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
            "name": "load_skill",
            "function": {
                "name": "load_skill",
                "description": "Load the full instructions for an available skill by skill_id. Use this only when the task clearly matches a skill from the available skills catalog.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "skill_id": { "type": "string", "description": "Skill ID from the available skills catalog" }
                    },
                    "required": ["skill_id"],
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
            "name": "symbol_trace",
            "function": {
                "name": "symbol_trace",
                "description": "Trace bounded multi-hop incoming/outgoing symbol relationships from one seed symbol. Returns visited symbols, edges, hop depth, truncation status, and unresolved edge counts without claiming type-aware call graph precision. A symbol_id must be copied exactly from a prior result; invented ids are rejected with an error.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "symbol_id": { "type": "string", "description": "Stable symbol ID" },
                        "path": { "type": "string", "description": "Optional file path when resolving by name" },
                        "qualified_name": { "type": "string", "description": "Optional exact qualified name" },
                        "name": { "type": "string", "description": "Optional simple symbol name" },
                        "direction": { "type": "string", "description": "incoming, outgoing, or both; defaults to both" },
                        "relationship_type": { "type": "string", "description": "Optional single edge kind: call, import, export, extends, implements, contains, usage, uses_type, reads_env, or handles" },
                        "relationships": { "type": "array", "items": { "type": "string" }, "description": "Optional edge kind list" },
                        "depth": { "type": "integer", "description": "Optional traversal depth, capped at 4" },
                        "edge_limit": { "type": "integer", "description": "Optional total edge cap, capped at 200" },
                        "per_node_limit": { "type": "integer", "description": "Optional per-node per-relationship edge cap, capped at 50" }
                    },
                    "required": [],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "symbol_path",
            "function": {
                "name": "symbol_path",
                "description": "Find the lowest-cost bounded path between two symbols. Path cost favors strong relationship types, syntax evidence, and high-confidence target resolution. Ambiguous endpoints are rejected instead of guessed. Symbol-id selectors must be copied exactly from a prior result; invented ids are rejected with an error.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string", "description": "Source symbol name or qualified name; use an explicit selector when ambiguous" },
                        "source_symbol_id": { "type": "string", "description": "Exact source stable symbol ID" },
                        "source_path": { "type": "string", "description": "Source file path used with source_name or source_qualified_name" },
                        "source_name": { "type": "string", "description": "Source simple name within source_path" },
                        "source_qualified_name": { "type": "string", "description": "Source qualified name within source_path" },
                        "target": { "type": "string", "description": "Target symbol name or qualified name; use an explicit selector when ambiguous" },
                        "target_symbol_id": { "type": "string", "description": "Exact target stable symbol ID" },
                        "target_path": { "type": "string", "description": "Target file path used with target_name or target_qualified_name" },
                        "target_name": { "type": "string", "description": "Target simple name within target_path" },
                        "target_qualified_name": { "type": "string", "description": "Target qualified name within target_path" },
                        "direction": { "type": "string", "description": "outgoing, incoming, or both; defaults to both" },
                        "relationships": { "type": "array", "items": { "type": "string" }, "description": "Optional relationship kinds" },
                        "max_hops": { "type": "integer", "description": "Maximum path length; defaults to 6 and is capped at 8" },
                        "edge_limit": { "type": "integer", "description": "Search edge budget; defaults to 300 and is capped at 500" },
                        "per_node_limit": { "type": "integer", "description": "Per-node expansion cap; defaults to 20 and is capped at 50" },
                        "min_confidence": { "type": "number", "description": "Minimum effective edge confidence from 0 to 1; defaults to 0.5" }
                    },
                    "required": [],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "symbol_query",
            "function": {
                "name": "symbol_query",
                "description": "Return compact seed-connected symbol neighborhoods for a natural-language code question, plus rationale/design-document anchors owned by returned symbols. Uses ranked lexical seeds, bounded traversal, confidence filtering, and explicit node/edge/token budgets.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Codebase question or investigation task" },
                        "direction": { "type": "string", "description": "incoming, outgoing, or both; defaults to both" },
                        "relationships": { "type": "array", "items": { "type": "string" }, "description": "Optional relationship kinds" },
                        "depth": { "type": "integer", "description": "Traversal depth; defaults to 2 and is capped at 4" },
                        "max_seeds": { "type": "integer", "description": "Maximum ranked seed symbols; defaults to 3 and is capped at 5" },
                        "node_limit": { "type": "integer", "description": "Maximum returned nodes before token-budget adjustment; capped at 200" },
                        "edge_limit": { "type": "integer", "description": "Maximum returned edges before token-budget adjustment; capped at 300" },
                        "per_node_limit": { "type": "integer", "description": "Per-node expansion cap; defaults to 16 and is capped at 50" },
                        "min_confidence": { "type": "number", "description": "Minimum effective edge confidence from 0 to 1; defaults to 0.5" },
                        "token_budget": { "type": "integer", "description": "Approximate output token budget; defaults to 2000, minimum 400, cap 8000" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "symbol_architecture",
            "function": {
                "name": "symbol_architecture",
                "description": "Build a compressed confidence-weighted file/module graph for a workspace or directory scope. Returns inferred communities, central hubs, cross-community bridge modules/edges, and bounded aggregate edges. Communities are deterministic graph-analysis hints, not declared package boundaries.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "scope": { "type": "string", "description": "Optional workspace-relative directory scope; a file selects its containing directory" },
                        "relationship_type": { "type": "string", "description": "Optional single relationship kind" },
                        "relationships": { "type": "array", "items": { "type": "string" }, "description": "Optional relationship kinds; defaults to code dependency edges and excludes containment/environment-key noise" },
                        "min_confidence": { "type": "number", "description": "Minimum effective edge confidence from 0 to 1; defaults to 0.5" },
                        "max_modules": { "type": "integer", "description": "Candidate module cap before token-budget adjustment; defaults to 160, capped at 1000" },
                        "max_edges": { "type": "integer", "description": "Aggregate module-edge cap before token-budget adjustment; defaults to 320, capped at 2000" },
                        "max_communities": { "type": "integer", "description": "Returned inferred-community cap; defaults to 20, capped at 50" },
                        "token_budget": { "type": "integer", "description": "Approximate output token budget; defaults to 5000, minimum 1000, cap 12000" }
                    },
                    "required": [],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "symbol_schema",
            "function": {
                "name": "symbol_schema",
                "description": "Return compact Symbols Index coverage and schema counts: indexed files by extension/language/support level, symbols by type, relationship integrity stats, unresolved relationship targets, and semantic anchor counts. Optional path scopes counts to one file or directory and includes root-vs-scoped totals. Use before trusting broad or empty symbol searches.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Optional file or directory path scope, such as src/app or src/app/page.tsx" },
                        "scope": { "type": "string", "description": "Alias for path; optional file or directory scope" }
                    },
                    "required": [],
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
                "description": "Execute command (requires approval). Prefer structured form with program+args. Use command only when shell behavior is required. For long-running jobs (dev servers, watchers, builds) set background:true so control returns to you after wait_ms with a session_id you can drive via command_session.",
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
                        "cwd": { "type": "string", "description": "Working directory" },
                        "background": { "type": "boolean", "description": "Run detached: if the command is still running after wait_ms, return a session_id instead of blocking. Poll/write/kill it with command_session. Default false." },
                        "wait_ms": { "type": "number", "description": "How long (ms) to wait for the command to finish before backgrounding it. Default 10000, clamped 250..30000. Only used when background is true." }
                    },
                    "required": [],
                    "additionalProperties": false
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "name": "command_session",
            "function": {
                "name": "command_session",
                "description": "Interact with a background command started by run_command(background:true). Poll for new output (default), write to its stdin, send Ctrl-C, or force-kill it. No approval needed — the command was approved when it started.",
                "strict": false,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "session_id": { "type": "string", "description": "The session_id returned by a backgrounded run_command." },
                        "input": { "type": "string", "description": "Bytes to write to the process stdin. Empty (default) = poll without writing. Use \"\\u0003\" to send Ctrl-C (interrupt)." },
                        "kill": { "type": "boolean", "description": "Force-kill the process, then return its final output. Default false." },
                        "wait_ms": { "type": "number", "description": "How long (ms) to wait for new output before returning. Default 3000, clamped 250..30000." }
                    },
                    "required": ["session_id"],
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
    fn includes_confidence_aware_graph_navigation_definitions() {
        let defs = get_tool_definitions();
        let names = defs
            .iter()
            .filter_map(|value| value.get("name").and_then(|value| value.as_str()))
            .collect::<Vec<_>>();

        assert!(names.contains(&"symbol_path"));
        assert!(names.contains(&"symbol_query"));
        assert!(names.contains(&"symbol_architecture"));
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
    fn excludes_legacy_project_index_tools_from_model_definitions() {
        let defs = get_tool_definitions();
        let names = defs
            .iter()
            .filter_map(|value| value.get("name").and_then(|v| v.as_str()))
            .collect::<Vec<&str>>();

        assert!(!names.contains(&"get_project_index_overview"));
        assert!(!names.contains(&"get_project_index_chunk"));
        assert!(names.contains(&"fast_context"));
        assert!(names.contains(&"load_skill"));
    }

    #[test]
    fn symbol_schema_definition_includes_scope_alias() {
        let defs = get_tool_definitions();
        let symbol_schema = defs
            .iter()
            .find(|value| value.get("name").and_then(|v| v.as_str()) == Some("symbol_schema"))
            .expect("symbol_schema tool definition");
        let properties = &symbol_schema["function"]["parameters"]["properties"];

        assert!(properties.get("path").is_some());
        assert!(properties.get("scope").is_some());
    }

    #[test]
    fn low_signal_toggle_present_on_reference_shaped_tools() {
        let defs = get_tool_definitions();
        for tool in ["symbol_references", "symbol_graph", "edit_impact"] {
            let def = defs
                .iter()
                .find(|value| value.get("name").and_then(|v| v.as_str()) == Some(tool))
                .unwrap_or_else(|| panic!("{tool} tool definition"));
            let properties = &def["function"]["parameters"]["properties"];

            assert!(
                properties.get("include_low_signal").is_some(),
                "{tool} is missing include_low_signal"
            );
        }
    }

    #[test]
    fn semantic_anchor_search_definition_includes_mode_enum() {
        let defs = get_tool_definitions();
        let anchor_search = defs
            .iter()
            .find(|value| {
                value.get("name").and_then(|v| v.as_str()) == Some("semantic_anchor_search")
            })
            .expect("semantic_anchor_search tool definition");
        let mode = &anchor_search["function"]["parameters"]["properties"]["mode"];

        assert_eq!(mode["type"].as_str(), Some("string"));
        let values = mode["enum"]
            .as_array()
            .expect("mode enum values")
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<&str>>();
        assert_eq!(values, vec!["phrase", "all_terms", "any_terms"]);
    }

    #[test]
    fn compact_navigation_options_and_reference_selectors_are_exposed() {
        let defs = get_tool_definitions();
        let properties = |tool: &str| {
            &defs
                .iter()
                .find(|value| value.get("name").and_then(|value| value.as_str()) == Some(tool))
                .unwrap_or_else(|| panic!("{tool} tool definition"))["function"]["parameters"]
                ["properties"]
        };

        assert!(properties("fast_context").get("detail").is_some());
        for key in ["nested", "include_imports", "include_locals"] {
            assert!(properties("symbol_outline").get(key).is_some());
        }
        for key in ["query", "direction", "relationship_kinds", "cursor"] {
            assert!(properties("symbol_references").get(key).is_some());
        }
    }

    #[test]
    fn tool_descriptions_stay_within_a_bounded_prompt_budget() {
        const DESCRIPTION_BUDGET_BYTES: usize = 24 * 1024;
        let total = get_tool_definitions()
            .iter()
            .filter_map(|definition| definition["function"]["description"].as_str())
            .map(str::len)
            .sum::<usize>();
        assert!(
            total <= DESCRIPTION_BUDGET_BYTES,
            "tool descriptions use {total} bytes; budget is {DESCRIPTION_BUDGET_BYTES}"
        );
    }

    #[test]
    fn symbol_tool_descriptions_teach_routing_and_id_handoff() {
        let defs = get_tool_definitions();
        let description = |tool: &str| -> String {
            defs.iter()
                .find(|value| value.get("name").and_then(|v| v.as_str()) == Some(tool))
                .unwrap_or_else(|| panic!("{tool} tool definition"))["function"]["description"]
                .as_str()
                .unwrap_or_else(|| panic!("{tool} description"))
                .to_string()
        };

        let search = description("symbol_search");
        assert!(search.contains("use before grep"));
        assert!(search.contains("never construct an id"));

        let outline = description("symbol_outline");
        assert!(outline.contains("file is known but the relevant range is not"));

        let references = description("symbol_references");
        assert!(references.contains("incoming call"));
        assert!(references.contains("incoming implements"));
        assert!(references.contains("never construct an id"));
        assert!(references.contains("include_low_signal"));
        assert!(references.contains("low_signal_suppressed"));

        for tool in [
            "symbol_resolve",
            "symbol_related",
            "symbol_graph",
            "symbol_trace",
            "symbol_path",
        ] {
            assert!(
                description(tool).contains("invented ids are rejected"),
                "{tool} description is missing the invented-id rejection sentence"
            );
        }
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
