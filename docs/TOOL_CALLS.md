# Zaguán Blade Tool Calls Reference

This document lists the tool calls that are actually executable in the local `zblade` runtime.

It intentionally excludes:

- **ZLP-related tooling**
- **Anything handled server-side by `zcoderd`**
- **Schema-only entries that are not currently executed by `zblade`**

The authoritative implementation lives in `src-tauri/src/tools.rs`.

---

## Scope

These tools are the ones `zblade` can execute locally in its own tool executor.

Excluded server-side tools include:

- `ask_followup_question`
- `attempt_completion`
- `new_task`
- `generate_image`
- `todo_write`

Also excluded from this document:

- `symbol_references` because it is present in tool schema definitions but is **not currently dispatched by the local executor**.

---

## File Tools

### `read_file`

Read the full contents of a file.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path |

**Accepted aliases for `path`:** `file_path`, `filepath`, `filename`

**Example:**

```json
{
  "path": "src/main.rs"
}
```

### `read_file_range`

Read a specific line range from a file, with optional surrounding context.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path |
| `start_line` | integer | No | Start line, 1-indexed. Defaults to `1`. |
| `end_line` | integer | No | End line, 1-indexed. Defaults to end of file. |
| `context_lines` | integer | No | Extra lines before and after the requested range. Defaults to `0`. |

**Accepted aliases for `path`:** `file_path`, `filepath`, `filename`

**Example:**

```json
{
  "path": "src/lib.rs",
  "start_line": 50,
  "end_line": 100,
  "context_lines": 3
}
```

### `write_file`

Write content to a file. Parent directories are created if needed.

### `create_file`

Alias of `write_file`.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | Target file path |
| `content` | string | Yes | File contents |

**Accepted aliases:**

- `path`: `file_path`, `filepath`, `filename`
- `content`: `contents`, `text`, `data`

**Example:**

```json
{
  "path": "src/new_module.rs",
  "content": "pub fn hello() {\n    println!(\"Hello!\");\n}\n"
}
```

### `edit_file`

Legacy single search/replace edit tool.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path |
| `old_content` | string | Yes | Text to find |
| `new_content` | string | Yes | Replacement text |

**Accepted aliases:**

- `path`: `file_path`, `filepath`, `filename`
- `old_content`: `old`, `from`
- `new_content`: `new`, `to`

### `apply_patch`

Preferred patch/edit tool for search/replace edits.

### `apply_edit`

Alias of `apply_patch`.

Supports both a single replacement and an atomic multi-patch mode.

**Single-patch parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path |
| `old_text` | string | Yes | Exact text to replace |
| `new_text` | string | Yes | Replacement text |

**Multi-patch parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path |
| `patches` | array | Yes | Array of patch objects |

Each patch object supports:

- `old_text` — required
- `new_text` — required
- `start_line` — optional hint
- `end_line` — optional hint

**Accepted aliases in single-patch mode:**

- `path`: `file_path`, `filepath`, `filename`
- `old_text`: `old_content`, `old`, `from`
- `new_text`: `new_content`, `new`, `to`

**Important behavior:**

- Matching is exact.
- If the match is ambiguous, the edit fails.
- Multi-patch application is atomic: if one patch fails validation, none are applied.

### `delete_file`

Delete a file, or a directory when `recursive: true` is provided.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | Path to delete |
| `recursive` | boolean | No | Required when deleting a directory |

### `move_file`

Move or rename a file.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `source` | string | Yes | Source path |
| `destination` | string | Yes | Destination path |

### `copy_file`

Copy a file or recursively copy a directory.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `source` | string | Yes | Source path |
| `destination` | string | Yes | Destination path |

### `create_directory`

Create a directory and any missing parent directories.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | Directory path |

### `get_file_info`

Return basic filesystem metadata.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | Path to inspect |

**Returns:** JSON including `path`, `size`, `is_directory`, `is_file`, `modified`, and `readonly`.

---

## Directory and Search Tools

### `list_dir`

List directory contents using a compact tree-like view.

### `list_directory`

Alias of `list_dir`.

This is implemented by forwarding to `get_workspace_structure` with a default shallow depth.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | No | Directory path. Defaults to `.`. |

**Notes:**

- `list_dir` / `list_directory` exists mainly as a compatibility entry point.
- The underlying structured traversal behavior is defined by `get_workspace_structure`.
- Prefer `get_workspace_structure` when you need explicit traversal controls.

### `get_workspace_structure`

Return a tree view of the workspace or a subdirectory.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | No | Starting path. Defaults to `.`. |
| `depth` | integer | No | Max traversal depth. Defaults to `2`. |
| `limit` | integer | No | Max returned entries. Defaults to `50`, capped at `200`. |

**Accepted aliases for `path`:** `dir`, `directory`

**Behavior notes:**

- Hidden files are skipped.
- Common heavy/generated directories are skipped.
- Gitignored paths are filtered when project settings enable that behavior.

### `find_files`

Find files by substring match on filename.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `pattern` | string | Yes | Substring to match in entry names |
| `path` | string | No | Search root inside the workspace |
| `max_depth` | integer | No | Optional max traversal depth |

### `find_files_glob`

Find files with a glob pattern.

### `glob`

Alias of `find_files_glob`.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `pattern` | string | Yes | Glob pattern |
| `path` | string | No | Optional base path |
| `case_sensitive` | boolean | No | Whether matching is case-sensitive |

**Accepted aliases:**

- `pattern`: `glob`

### `grep_search`

Search file contents with a regular expression.

### `rg`

Alias of `grep_search`.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `pattern` | string | Yes | Regex pattern |
| `path` | string | No | Search root. Defaults to `.`. |
| `include_dependencies` | boolean | No | Include dependency directories like `node_modules` and `vendor` |
| `timeout_ms` | integer | No | Timeout used when timeout enforcement is enabled |

**Accepted aliases:**

- `pattern`: `query`, `regex`
- `path`: `dir`, `directory`

**Behavior notes:**

- Returns matches in `path:line:text` form on success.
- If timeout enforcement is enabled and the search times out, it returns structured JSON with partial results and a hint.

### `codebase_search`

Search the codebase with regex and return matches with surrounding context.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `query` | string | Yes | Regex pattern |
| `file_pattern` | string | No | Comma-separated filename patterns |
| `max_results` | integer | No | Max number of matches to return. Defaults to `50`. |

---

## Editor and UI Interaction Tools

### `get_editor_state`

Return the current local editor context.

**Parameters:** None.

**Returns:** JSON including:

- `active_file`
- `open_files`
- `active_tab_index`
- `cursor_line`
- `cursor_column`
- `selection_start_line`
- `selection_end_line`

The returned text may also include human-readable helper guidance for the active file and cursor location.

### `open_file`

Emit an editor action that opens a file, optionally at a line.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File to open |
| `line` | integer | No | Line number to jump to |

**Note:** This returns an action payload for the frontend to intercept.

### `goto_line`

Emit an editor action that moves the cursor in the current file.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `line` | integer | Yes | Target line |
| `column` | integer | No | Optional target column |

### `get_selection`

Request the current selection.

**Parameters:** None.

**Current status:** implemented only as a placeholder action payload. It does **not** currently return the true selected text.

### `replace_selection`

Emit an editor action that replaces the current selection.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `content` | string | Yes | Replacement content |

### `insert_at_cursor`

Emit an editor action that inserts content at the current cursor position.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `content` | string | Yes | Text to insert |

---

## Local Code-Intelligence Tools

These tools are local to `zblade`. They are **not ZLP tools** and do not require `zcoderd`, but they do depend on `zblade`'s local language service / symbol index being available.

### `fast_context`

Assemble targeted context for broad, ambiguous, multi-file, or unfamiliar coding tasks before reading many files. Returns structured project context, ranked primary files, symbol and semantic-anchor evidence, indexed Markdown sections, related files, optional impact hints, index health, confidence, suggested read ranges, and next steps.

Legacy project-index Markdown is not included by default. Use `include_project_index_min: true` only as an explicit fallback during the project-index migration.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `query` | string | Yes | User task or investigation query |
| `queries` | array | No | Additional search queries |
| `intent` | string | No | Optional task intent such as `bug_fix`, `feature`, `refactor`, or `docs` |
| `max_results` | integer | No | Max primary files |
| `include_tests` | boolean | No | Whether to include likely tests |
| `include_docs` | boolean | No | Whether to include related docs |
| `include_memory` | boolean | No | Whether to include local project memories |
| `include_project_index_min` | boolean | No | Legacy fallback only. Defaults to `false`. |

### `symbol_search`

Search indexed symbols by name or qualified name.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `query` | string | Yes | Symbol query |
| `path` | string | No | Restrict to a file |
| `kind` | string | No | Symbol kind filter |
| `limit` | integer | No | Max results. Defaults to `20`, capped at `100`. |

**Accepted aliases:**

- `path`: `file`, `file_path`
- `kind`: `symbol_type`

### `symbol_resolve`

Resolve one symbol by ID, or by file plus name.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `symbol_id` | string | Conditionally | Stable symbol ID |
| `path` | string | Conditionally | Required when resolving by file-scoped lookup |
| `qualified_name` | string | No | Exact qualified name |
| `name` | string | No | Simple symbol name |

**Accepted aliases:**

- `symbol_id`: `id`
- `path`: `file`, `file_path`

**Rule:** provide either `symbol_id`, or a `path` with a symbol name selector.

### `symbol_related`

Return evidence-backed symbols related to one seed symbol. Relatedness includes direct graph edges, same-module exports, importers of the seed symbol's module, and consumers of sibling exports from that module.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `symbol_id` | string | Conditionally | Stable symbol ID |
| `path` | string | Conditionally | File path when resolving by file-scoped lookup |
| `qualified_name` | string | No | Exact qualified name |
| `name` | string | No | Simple symbol name |
| `limit` | integer | No | Max related symbols. Defaults to `24`, capped at `100`. |

**Accepted aliases:**

- `symbol_id`: `id`
- `path`: `file`, `file_path`

**Rule:** provide either `symbol_id`, or a `path` with a symbol name selector.

### `symbol_outline`

Return the hierarchical symbol outline for one file.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path |

**Accepted aliases for `path`:** `file`, `file_path`

### `symbol_graph`

Return incoming and outgoing graph edges for a symbol.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `symbol_id` | string | Conditionally | Stable symbol ID |
| `path` | string | Conditionally | File path when resolving by file/name |
| `qualified_name` | string | No | Exact qualified name |
| `name` | string | No | Simple symbol name |
| `relationship_type` | string | No | Edge type. Defaults to `call`. |
| `limit` | integer | No | Max edges. Defaults to `20`, capped at `100`. |

**Accepted aliases:**

- `symbol_id`: `id`
- `path`: `file`, `file_path`
- `relationship_type`: `edge_kind`, `kind`

---

## Project Index Tools

These are legacy fallback tools during the project-index migration. They remain locally executable for compatibility, but they are no longer advertised in normal model-facing tool schemas. Prefer `fast_context` for first-pass orientation and targeted symbol-aware context. Use these only when explicitly investigating the generated `.zblade/context/project_index.md` artifact or when compatibility requires it.

### `get_project_index_overview`

Legacy fallback only. Read a compact overview window from the local project index when explicitly investigating generated project-index Markdown.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | No | Optional workspace sub-root |
| `max_chars` | integer | No | Character budget. Defaults to `6000`, capped at `12000`. |
| `offset` | integer | No | Character offset |

### `get_project_index_chunk`

Legacy fallback only. Read a deterministic paged chunk from the local project index when explicit compatibility paging is required.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | No | Optional workspace sub-root |
| `offset` | integer | No | Character offset |
| `max_chars` | integer | No | Character budget. Defaults to `4000`, capped at `8000`. |

---

## Composite Read-Only Tools

These tools are supported by `zblade` locally.

By default, schema exposure may be gated by model family and the `composite_tools_enabled` feature flag, but the local executor does support them.

### `read_many_files`

Read multiple files selected by glob patterns in a single bounded call.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `paths` | array of strings | Yes | Include globs |
| `exclude` | array of strings | No | Exclude globs |
| `max_files` | integer | No | Max files. Defaults to `100`, capped at `500`. |
| `max_bytes_per_file` | integer | No | Per-file byte limit. Defaults to `65536`, capped at `524288`. |
| `include_line_numbers` | boolean | No | Include line numbers in returned content |

**Accepted aliases for include globs:** `globs`, `patterns`

### `batch`

Execute multiple read-only tool calls concurrently with all-settled behavior.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `calls` | array | Yes | Array of tool call objects |
| `max_parallel` | integer | No | Defaults to `8`, capped at `16` |
| `fail_fast` | boolean | No | Stop queued work after first failure |
| `ordered` | boolean | No | Preserve input order. Defaults to `true`. |
| `cancel_after_ms` | integer | No | Optional total budget |

Each call object supports:

- `tool` or `name`
- `arguments`

**Important behavior:**

- Only read-only tools are allowed.
- `batch` itself and `run_command` are explicitly blocked inside `batch`.

### `codebase_investigator`

Run a bounded read-only investigation pass and return structured findings.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `objective` | string | Yes | Investigation goal |
| `scope` | array of strings | No | Glob scope. Defaults to `**/*`. |
| `max_turns` | integer | No | Defaults to `8`, capped at `16` |
| `max_tool_calls` | integer | No | Defaults to `40`, capped at `120` |
| `output_format` | string | No | `json` or `markdown` |
| `cancel_after_ms` | integer | No | Optional total budget |

---

## Command Execution

### `run_command`

Execute a command in the workspace. This requires approval in normal AI workflows.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| `command` | string | Conditionally | Legacy shell command line |
| `program` | string | Conditionally | Executable path/name for structured execution |
| `args` | array of strings | No | Structured arguments when using `program` |
| `shell` | boolean | No | Force shell execution when using `program` |
| `cwd` | string | No | Working directory |
| `blocking` | boolean | No | Defaults to `true` |
| `wait_ms_before_async` | integer | No | Startup wait when non-blocking |

**Accepted aliases / alternate casing:**

- `command`: `Command`, `command_line`, `CommandLine`
- `program`: `Program`
- `args`: `Args`
- `shell`: `Shell`
- `cwd`: `Cwd`
- `blocking`: `Blocking`
- `wait_ms_before_async`: `WaitMsBeforeAsync`

**Rule:** provide either `command` or `program`.

**Behavior notes:**

- If `program` is used, shell execution defaults to `false` unless explicitly enabled.
- If `command` is used, shell execution is enabled.

---

## General Notes

### Result truncation

Large tool results may be truncated.

- **Max size:** `50 KB`
- **Max lines:** `2000`

When truncation happens, the executor keeps a head/tail summary rather than returning the entire payload.

### Path safety

Paths are generally constrained to the current workspace.

- Relative paths are resolved from the workspace root.
- Absolute paths must still resolve inside the workspace when validation is enforced.

### Local vs advertised tools

This file documents what `zblade` can execute locally.

That is not always identical to what every model sees in advertised tool schemas:

- Some compatibility aliases are executor-only.
- Some composite tools are feature-flag and model-family gated for schema exposure.
- Some schema entries may exist before local execution support is complete.

---

## Quick Reference

### Locally executable non-server-side tools

- `read_file`
- `read_file_range`
- `write_file`
- `create_file`
- `edit_file`
- `apply_patch`
- `apply_edit`
- `delete_file`
- `move_file`
- `copy_file`
- `create_directory`
- `get_file_info`
- `list_dir`
- `list_directory`
- `get_workspace_structure`
- `find_files`
- `find_files_glob`
- `glob`
- `grep_search`
- `rg`
- `codebase_search`
- `get_editor_state`
- `open_file`
- `goto_line`
- `get_selection`
- `replace_selection`
- `insert_at_cursor`
- `fast_context`
- `symbol_search`
- `symbol_resolve`
- `symbol_related`
- `symbol_outline`
- `symbol_graph`
- `get_project_index_overview`
- `get_project_index_chunk`
- `read_many_files`
- `batch`
- `codebase_investigator`
- `run_command`
