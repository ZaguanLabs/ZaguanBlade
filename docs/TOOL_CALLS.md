# Zaguán Blade Tool Calls Reference

This document describes the tool calls that the current Blade desktop runtime can handle locally.

Authoritative code paths:

- Model-facing schemas: `src-tauri/src/ai_workflow/tool_defs.rs`
- Local executor: `src-tauri/src/tools.rs`
- `run_command` approval and execution: `src-tauri/src/ai_workflow.rs`
- Background command sessions: `src-tauri/src/terminal.rs` (registry), `src-tauri/src/tools.rs` (`command_session`)

Model-facing schemas are not identical to every compatibility alias in the executor. Some tools remain executable for legacy or fallback compatibility even when they are not advertised to normal model turns.

## Server-Side Tools

These are handled by Zaguán Coder Daemon and should not be executed by Blade's local executor:

- `ask_followup_question`
- `attempt_completion`
- `new_task`
- `generate_image`
- `todo_write`

If Blade receives one of these for local execution, it reports a protocol error.

## Tool Result Limits

Local tool output is truncated for large results before it is sent back to the model:

- Maximum result size: 50 KB
- Maximum result lines: 2,000
- Truncation keeps a head and tail preview

`read_many_files`, `grep_search`, and several index tools also return their own count, truncation, health, or timing metadata.

## Editor Context Tools

### `get_editor_state`

Returns current editor context: active file, open files, active tab index, cursor position, and selection range when available.

Parameters: none.

### `open_file`

Request that the UI open a file.

Parameters:

| Name | Type | Required | Notes |
|------|------|----------|-------|
| `path` | string | Yes | Workspace path |
| `line` | integer | No | Optional target line |
| `column` | integer | No | Optional target column |

### `goto_line`

Request that the UI navigate to a line in the active file.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `line` | integer | Yes |
| `column` | integer | No |

### `get_selection`

Returns current selection metadata from editor state when available.

Parameters: none.

### `replace_selection`

Request replacement of the current editor selection.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `text` | string | Yes |

### `insert_at_cursor`

Request insertion at the current editor cursor.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `text` | string | Yes |

## File Read Tools

### `read_file`

Reads a full file.

Parameters:

| Name | Type | Required | Aliases |
|------|------|----------|---------|
| `path` | string | Yes | `file_path`, `filepath`, `filename` |

### `read_file_range`

Reads a 1-indexed line range with optional surrounding context.

Parameters:

| Name | Type | Required | Notes |
|------|------|----------|-------|
| `path` | string | Yes | aliases: `file_path`, `filepath`, `filename` |
| `start_line` | integer | No | Defaults to `1` |
| `end_line` | integer | No | Defaults to end of file |
| `context_lines` | integer | No | Defaults to `0` |

### `read_many_files`

Reads many files matched by glob patterns and returns JSON with per-file content and summary metadata.

Parameters:

| Name | Type | Required | Notes |
|------|------|----------|-------|
| `paths` | string[] | Yes | aliases: `globs`, `patterns`; single aliases: `path`, `pattern`, `glob` |
| `exclude` | string[] | No | alias: `excludes` |
| `max_files` | integer | No | default `100`, cap `500` |
| `max_bytes_per_file` | integer | No | cap `512 KB` |
| `include_line_numbers` | boolean | No | defaults to `true` |

## File Write and Edit Tools

Writes and edits create history snapshots and update Blade's uncommitted-change tracking when executed inside the app.

### `write_file`

Writes content to a file, creating parent directories when needed.

Aliases: `write_file_validated`, `create_file`, `write_to_file`.

Parameters:

| Name | Type | Required | Aliases |
|------|------|----------|---------|
| `path` | string | Yes | `file_path`, `filepath`, `filename` |
| `content` | string | Yes | `contents`, `text`, `data` |

### `edit_file`

Legacy single search/replace edit.

Parameters:

| Name | Type | Required | Aliases |
|------|------|----------|---------|
| `path` | string | Yes | `file_path`, `filepath`, `filename` |
| `old_content` | string | Yes | `old`, `from` |
| `new_content` | string | Yes | `new`, `to` |

### `apply_patch`

Preferred exact search/replace edit tool.

Aliases: `apply_edit`, `apply_patch_validated`, `replace_file_content`, `multi_replace_file_content`.

Single-patch parameters:

| Name | Type | Required | Aliases |
|------|------|----------|---------|
| `path` | string | Yes | `file_path`, `filepath`, `filename` |
| `old_text` | string | Yes | `old_content`, `old`, `from` |
| `new_text` | string | Yes | `new_content`, `new`, `to` |
| `start_line` | integer | No | Optional disambiguation hint |
| `end_line` | integer | No | Optional disambiguation hint |

Multi-patch parameters:

| Name | Type | Required | Notes |
|------|------|----------|-------|
| `path` | string | Yes | Target file |
| `patches` | array | Yes | Atomic list of `{ old_text, new_text, start_line?, end_line? }` |

Behavior:

- Matching is exact.
- Ambiguous matches fail.
- Multi-patch mode is atomic: if one patch fails, no changes are written.

### Semantic Patch Mode

`apply_patch` also accepts a structured `semantic_patch` object, or `patch` object, handled by the semantic patch applier. This can update a primary file and additional generated changes atomically through the language service.

## Filesystem Tools

### `create_directory`

Creates a directory and missing parents.

Parameters: `path` string, required.

### `delete_file`

Deletes a file. Deletes a directory only when `recursive: true`.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `path` | string | Yes |
| `recursive` | boolean | No |

### `move_file`

Moves or renames a file.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `source` | string | Yes |
| `destination` | string | Yes |

### `copy_file`

Copies a file or recursively copies a directory.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `source` | string | Yes |
| `destination` | string | Yes |

### `get_file_info`

Returns JSON metadata: path, size, `is_directory`, `is_file`, modified timestamp, and readonly flag.

Parameters: `path` string, required.

## Directory and Search Tools

### `list_dir`

Lists immediate directory contents as JSON.

Alias: `list_directory`.

Parameters:

| Name | Type | Required | Notes |
|------|------|----------|-------|
| `path` | string | No | Defaults to `.` |

### `get_workspace_structure`

Returns a compact tree-like directory view.

Parameters:

| Name | Type | Required | Notes |
|------|------|----------|-------|
| `path` | string | No | aliases: `dir`, `directory`; defaults to `.` |
| `depth` | integer | No | default `2` |
| `limit` | integer | No | default `50`, cap `200` |

Behavior:

- Hidden files and directories are skipped.
- Heavy/generated directories such as `node_modules`, `.git`, `target`, `dist`, and `.zblade` are skipped.
- Gitignored paths are filtered unless project settings allow them.

### `find_files`

Finds files by substring match on entry name.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `pattern` | string | Yes |
| `path` | string | No |
| `max_depth` | integer | No |

### `find_files_glob`

Finds files with a glob pattern.

Alias: `glob`.

Parameters:

| Name | Type | Required | Aliases |
|------|------|----------|---------|
| `pattern` | string | Yes | `glob` |
| `path` | string | No | Base path |
| `case_sensitive` | boolean | No | Defaults to `false` |

### `grep_search`

Searches file contents with a regular expression.

Alias: `rg`.

Parameters:

| Name | Type | Required | Notes |
|------|------|----------|-------|
| `pattern` | string | Yes | aliases: `query`; regular expression |
| `path` | string | No | Defaults to `.` |
| `include_dependencies` | boolean | No | Include directories such as `node_modules` and `vendor` |
| `timeout_ms` | integer | No | default `8000`, min `500`, max `30000` when timeout enforcement is enabled |
| `max_results` | integer | No | alias: `limit`; capped at `20` |

### `codebase_search`

Legacy regex search that returns matching lines with nearby context.

Parameters:

| Name | Type | Required | Notes |
|------|------|----------|-------|
| `query` | string | Yes | Regular expression |
| `file_pattern` | string | No | Comma-separated filename patterns |
| `max_results` | integer | No | Defaults to `50` |

## Code Intelligence Tools

These tools require the language service and local code index to be available.

### `fast_context`

Plans broad or uncertain code tasks and returns targeted context, ranked files, symbol and semantic-anchor metadata, related files, index health, language-support metadata, a compact index schema summary, confidence, suggested ranges, and next steps.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `query` | string | Yes |
| `queries` | string[] | No |
| `intent` | string | No |
| `max_results` | integer | No |
| `include_tests` | boolean | No |
| `include_docs` | boolean | No |
| `include_memory` | boolean | No |
| `include_project_index_min` | boolean | No |

### `symbol_search`

Searches indexed symbols by name or qualified name. Results include code definitions plus partial scanner symbols such as CSS selectors/custom properties, markup `class`/`id` selectors, and JSON/YAML/TOML config key paths.

Responses include `_meta.language_support`, `_meta.search_health`, `_meta.index_health`, `_meta.filters`, and pagination fields such as `_meta.offset`, `_meta.limit`, `_meta.has_more`, and `_meta.total_lower_bound`. Empty results should only be treated as trustworthy when language support says the relevant file type is supported and the index is fresh enough for the task. Pagination totals are lower-bound counts over the bounded candidate set, not an expensive exact workspace-wide total.

Parameters:

| Name | Type | Required | Aliases |
|------|------|----------|---------|
| `query` | string | Yes | |
| `path` | string | No | `file`, `file_path` |
| `file_pattern` | string | No | comma-separated file path glob or substring filter; alias `path_pattern` |
| `name_pattern` | string | No | comma-separated symbol-name glob or substring filter |
| `qualified_name_pattern` | string | No | comma-separated qualified-name glob or substring filter; alias `qualified_pattern` |
| `kind` | string | No | `symbol_type` |
| `limit` | integer | No | cap `100` |
| `offset` | integer | No | zero-based pagination offset; cap `1000` |
| `include_connected` | boolean | No | compact one-hop connected-symbol preview for returned results; use `symbol_references` for one-hop expansion or `symbol_trace` for bounded multi-hop traversal |

### `semantic_anchor_search`

Searches indexed semantic anchors such as command names, event names, route-like strings, config keys, translation keys/text, and CSS/theme tokens. Use this for literals and anchors when structural symbol support is absent, shallow, or too narrow for the question.

Responses include `_meta.language_support` for the optional file filter.

Parameters:

| Name | Type | Required | Aliases |
|------|------|----------|---------|
| `query` | string | Yes | |
| `path` | string | No | `file`, `file_path` |
| `limit` | integer | No | cap `100` |

### `symbol_resolve`

Resolves a symbol by stable ID or by name within a file.

Responses include `_meta.language_support` for the resolved symbol file.

Parameters:

| Name | Type | Required | Aliases |
|------|------|----------|---------|
| `symbol_id` | string | No | `id` |
| `path` | string | No | `file`, `file_path` |
| `qualified_name` | string | No | |
| `name` | string | No | |

Requires either `symbol_id` or `path` plus `name` or `qualified_name`.

### `symbol_outline`

Returns a compact symbol inventory and optional hierarchy for one file. Unsupported files return explicit diagnostics instead of silently implying that no symbols exist. Partial scanner languages report shallow support in `_meta.language_support`.

Parameters:

| Name | Type | Required | Aliases |
|------|------|----------|---------|
| `path` | string | Yes | `file`, `file_path` |
| `max_symbols` | integer | No | `limit`; default `120`, cap `300` |
| `include_outline` | boolean | No | defaults to `false`; returns compact nodes only |
| `max_outline_nodes` | integer | No | `outline_limit`; default `120`, cap `500` |
| `max_outline_depth` | integer | No | `outline_depth`; default `4`, cap `12` |
| `include_docstrings` | boolean | No | `include_docs`; defaults to `false`; docstrings are truncated when included |

Response metadata includes `_meta.language_support`, `_meta.line_count` when the file index has stored line-count metadata, plus truncation fields such as `_meta.symbols_truncated` and `_meta.outline_truncated`.

### `symbol_related`

Returns symbols related to a seed symbol. Results can include structural graph relationships, same-module/module-import context, and bounded lexical-similarity fallbacks. Each related item includes `evidence`; lexical-similarity results are labeled as `structural: false` and `confidence: heuristic`, so they should be treated as navigation hints rather than graph truth.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `symbol_id` / `id` | string | No |
| `path` / `file` / `file_path` | string | No |
| `qualified_name` | string | No |
| `name` | string | No |
| `limit` | integer | No |

### `symbol_references`

Expands incoming and outgoing relationships for one symbol, or important symbols in a file.
Each edge reports two independent evidence dimensions: `observation` describes how the
relationship occurrence entered the index, while `resolution` reports the target-resolution
strategy, numeric confidence, receiver-type context, and whether the target resolved.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `symbol_id` / `id` | string | No |
| `path` / `file` / `file_path` | string | No |
| `qualified_name` | string | No |
| `name` | string | No |
| `relationship` | string | No |
| `relationships` | string[] | No |
| `limit` | integer | No |
| `max_symbols` | integer | No |

Relationship types include `call`, `import`, `export`, `extends`, `implements`, `contains`,
`usage`, `uses_type`, `reads_env`, and `handles`.

### `symbol_graph`

Returns one-hop incoming and outgoing graph edges for one symbol.
Edges include the same `observation` and `resolution` provenance as `symbol_references`.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `symbol_id` / `id` | string | No |
| `path` / `file` / `file_path` | string | No |
| `qualified_name` | string | No |
| `name` | string | No |
| `relationship` | string | No |
| `relationship_type` | string | No |
| `kind` | string | No |
| `limit` | integer | No |

Relationship types include `call`, `import`, `export`, `extends`, `implements`, `contains`,
`usage`, `uses_type`, `reads_env`, and `handles`.

### `symbol_trace`

Traces bounded multi-hop incoming and/or outgoing symbol relationships from one seed symbol. This is a structural index traversal, not a type-aware call graph. Responses include visited symbols, edges, hop depth, truncation status, and unresolved edge counts.
Every returned edge preserves its observation source and target-resolution strategy/confidence;
legacy resolved edges without stored provenance are explicitly labeled with unknown confidence.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `symbol_id` / `id` | string | No |
| `path` / `file` / `file_path` | string | No |
| `qualified_name` | string | No |
| `name` | string | No |
| `direction` | string | No, `incoming`, `outgoing`, or `both`; defaults to `both` |
| `relationship` | string | No |
| `relationship_type` | string | No |
| `relationships` | string[] | No |
| `depth` | integer | No, capped at `4` |
| `edge_limit` | integer | No, capped at `200`; alias `limit` |
| `per_node_limit` | integer | No, capped at `50` |

Relationship types include `call`, `import`, `export`, `extends`, `implements`, `contains`, and `usage`.

### `symbol_schema`

Returns compact Symbols Index coverage and schema counts. Use this before trusting broad searches, investigating empty search results, or deciding whether a language/file type has enough indexed coverage.

The response includes indexed file counts by extension, language, and support level; symbol counts by type; relationship integrity stats including unresolved targets; semantic anchor counts by kind; `_meta.index_health`; and `_meta.language_support`. When scoped with `path`, responses include root-vs-scoped totals under `schema.scope` and `_meta.scope`.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `path` / `scope` | string | No |

### `edit_impact`

Analyzes likely impact before editing a file or symbol. It follows bounded incoming relationship
paths transitively and inspects direct outgoing dependencies. Results include the evidence path
for each transitive hit, impacted files, likely tests, reference counts, risk, confidence, and
suggested read ranges.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `path` / `file` / `file_path` | string | No |
| `symbol_id` / `id` | string | No |
| `qualified_name` | string | No |
| `name` | string | No |
| `limit` | integer | No |
| `max_symbols` | integer | No |
| `depth` / `max_depth` | integer | No; default `2`, cap `4` |
| `edge_limit` | integer | No; default `160`, cap `400` |
| `per_node_limit` | integer | No; default `16`, cap `50` |

Requires either a target `path` or a symbol selector.

## Composite Tools

Composite tools may be omitted from model-facing schemas for models that are not expected to handle them reliably.

### `batch`

Executes multiple read-only tool calls and returns ordered all-settled results.

Parameters:

| Name | Type | Required | Notes |
|------|------|----------|-------|
| `calls` | array | Yes | Each item contains `tool` or `name`, plus `arguments` or `args` |
| `fail_fast` | boolean | No | Defaults to `false` |
| `ordered` | boolean | No | Defaults to `true` |

Allowed tools in `batch` are read-only only, including file reads, search, workspace structure, code-intelligence tools, project-index fallback tools, and `codebase_investigator`. `run_command`, nested `batch`, and mutation tools are rejected.

### `codebase_investigator`

Runs a bounded read-only investigation and returns structured findings with evidence references.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `objective` | string | Yes |
| `scope` | string[] | No |
| `max_turns` | integer | No |
| `max_tool_calls` | integer | No |
| `output_format` | string | No |
| `cancel_after_ms` | integer | No |

## Project Index Fallback Tools

These remain executable for compatibility but are not advertised in normal model-facing schemas. Prefer `fast_context`.

### `get_project_index_overview`

Returns a bounded overview of `.zblade/context/project_index.md`.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `path` | string | No |
| `max_chars` | integer | No | default `6000`, cap `12000` |

### `get_project_index_chunk`

Returns a character window from `.zblade/context/project_index.md`.

Parameters:

| Name | Type | Required |
|------|------|----------|
| `path` | string | No |
| `offset` | integer | No | default `0` |
| `max_chars` | integer | No | default `4000`, cap `8000` |

## Command Tools

### `run_command`

Executes a shell command or structured program invocation inside the workspace after approval. This is intercepted by the workflow layer, not dispatched through `tools.rs`.

Parameters:

| Name | Type | Required | Aliases / Notes |
|------|------|----------|-----------------|
| `command` | string | No | aliases: `Command`, `command_line`, `CommandLine`; uses shell mode |
| `program` | string | No | aliases: `Program`; structured non-shell mode |
| `args` | string[] | No | aliases: `Args`; used with `program` |
| `shell` | boolean | No | aliases: `Shell`; defaults to `false` for `program`, `true` for `command` |
| `cwd` | string | No | aliases: `Cwd`; must resolve inside the workspace |
| `blocking` | boolean | No | aliases: `Blocking`; defaults to `true` |
| `background` | boolean | No | alias: `Background`; model-facing alias for `blocking:false`; default `false` |
| `wait_ms` | integer | No | aliases: `wait_ms_before_async`, `WaitMsBeforeAsync`; clamped `250..30000`; defaults to `10000` when backgrounding |

Behavior:

- Requires user approval unless project **YOLO mode** is enabled.
- Rejects `cwd` outside the workspace.
- Captures stdout/stderr and exit status.
- Blocks one known irrelevant scan pattern: Python-file hunts in Rust workspaces with no Python project signals.

Background mode (`background: true`):

- The command runs for up to `wait_ms`. If it finishes in time, the result is returned as usual; otherwise the tool returns the output produced so far plus a `session_id` the model can drive with `command_session`.
- An explicit `background: false` never overrides a legacy `blocking: false` — both spellings map to the same detach path.
- Background output is buffered up to 1 MiB per job (oldest output is dropped, with an explicit truncation note — never silent). Blocking commands remain unbounded.
- A finished job is retained in the registry so a final poll still returns the output tail and the real exit code.
- The registry is capped at 32 jobs. Registering beyond the cap evicts the oldest already-exited session, or the oldest running one if none have exited; an evicted still-running job is killed.
- All background jobs are terminated on graceful app shutdown.
- Remote (zcoderd) models only see `background`/`wait_ms` and `command_session` when the client declares the `background_commands` capability in the authenticate handshake, preventing version skew with older clients. Local models use ZB's own schemas and always have them.

### `command_session`

Interacts with a background command started by `run_command(background: true)`: poll for new output (default), write to its stdin, send Ctrl-C, or force-kill it. Non-gated inline tool — no approval is needed because the underlying command was approved when it started.

Parameters:

| Name | Type | Required | Notes |
|------|------|----------|-------|
| `session_id` | string | Yes | the `session_id` returned by a backgrounded `run_command` |
| `input` | string | No | bytes written to the process stdin; empty (default) = poll only; the ETX byte (`U+0003`) sends Ctrl-C, and the literal spellings `\x03`, `\u0003`, and `^C` are normalized to it |
| `kill` | boolean | No | force-kill the process, then return its final output; default `false` |
| `wait_ms` | integer | No | how long to wait for new output before returning; default `3000`, clamped `250..30000` |

Behavior:

- Polls return only the output delta since the last poll, ANSI-stripped, with wall time and run/exit status. Returns early as soon as any new output arrives or the process exits.
- A stdin write to a process that already exited is not an error: the tool falls through to a poll and reports the exit and final tail instead.
- Unknown session IDs return an error (sessions may have been evicted or reaped — see the `run_command` background notes).
