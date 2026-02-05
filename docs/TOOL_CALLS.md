# Zaguán Blade Tool Calls Reference

This document describes the regular tool calls that Zaguán Blade supports. These are the tools you can add to your Local AI system prompts to extend usability.

> **Note:** This does not cover Blade-specific or ZLP (Zaguán Language Protocol) tools. These are standard file/editor tools for general AI coding assistance.

---

## File Operations

### `read_file`

Read the complete contents of a file.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path (relative to workspace or absolute) |

**Aliases:** `file_path`, `filepath`, `filename`

**Example:**
```json
{
  "path": "src/main.rs"
}
```

---

### `read_file_range`

Read a specific line range from a file with optional context.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path |
| `start_line` | integer | No | Start line (1-indexed, default: 1) |
| `end_line` | integer | No | End line (1-indexed, default: end of file) |
| `context_lines` | integer | No | Extra context lines before/after range (default: 0) |

**Example:**
```json
{
  "path": "src/lib.rs",
  "start_line": 50,
  "end_line": 100,
  "context_lines": 3
}
```

---

### `write_file` / `create_file`

Write content to a file. Creates parent directories if needed.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path |
| `content` | string | Yes | Content to write |

**Aliases for content:** `contents`, `text`, `data`

**Example:**
```json
{
  "path": "src/new_module.rs",
  "content": "pub fn hello() {\n    println!(\"Hello!\");\n}\n"
}
```

---

### `edit_file`

Apply a search/replace edit to a file (legacy tool).

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path |
| `old_content` | string | Yes | Text to find |
| `new_content` | string | Yes | Replacement text |

**Aliases:** `old`/`from` for old_content, `new`/`to` for new_content

**Example:**
```json
{
  "path": "src/main.rs",
  "old_content": "fn old_function()",
  "new_content": "fn new_function()"
}
```

---

### `apply_edit` / `apply_patch`

Apply search/replace edits with robust fuzzy matching. Supports both single patches and atomic multi-patch operations.

**Single Patch Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path |
| `old_text` | string | Yes | Text to find and replace |
| `new_text` | string | Yes | Replacement text |

**Multi-Patch Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path |
| `patches` | array | Yes | Array of patch objects |

Each patch object contains:
- `old_text` (string): Text to find
- `new_text` (string): Replacement text
- `start_line` (integer, optional): Hint for disambiguation
- `end_line` (integer, optional): Hint for disambiguation

**Single Patch Example:**
```json
{
  "path": "src/lib.rs",
  "old_text": "let x = 5;",
  "new_text": "let x = 10;"
}
```

**Multi-Patch Example:**
```json
{
  "path": "src/lib.rs",
  "patches": [
    {"old_text": "fn foo()", "new_text": "fn bar()"},
    {"old_text": "let a = 1;", "new_text": "let a = 2;"}
  ]
}
```

> **Note:** Multi-patch operations are atomic—all patches are validated before any are applied. If any patch fails, no changes are made.

---

### `delete_file`

Delete a file or directory.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | Path to delete |
| `recursive` | boolean | No | Required for directories (default: false) |

**Example:**
```json
{
  "path": "temp/old_file.txt"
}
```

---

### `move_file`

Move or rename a file.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `source` | string | Yes | Source path |
| `destination` | string | Yes | Destination path |

**Example:**
```json
{
  "source": "src/old_name.rs",
  "destination": "src/new_name.rs"
}
```

---

### `copy_file`

Copy a file or directory (recursive for directories).

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `source` | string | Yes | Source path |
| `destination` | string | Yes | Destination path |

**Example:**
```json
{
  "source": "templates/base.html",
  "destination": "src/templates/base.html"
}
```

---

### `get_file_info`

Get metadata about a file or directory.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | Path to inspect |

**Returns:** JSON with `path`, `size`, `is_directory`, `is_file`, `modified`, `readonly`

**Example:**
```json
{
  "path": "Cargo.toml"
}
```

---

### `create_directory`

Create a directory (and parent directories if needed).

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | Directory path to create |

**Example:**
```json
{
  "path": "src/modules/new_feature"
}
```

---

## Directory & Search Tools

### `list_directory` / `list_dir`

List directory contents with tree view.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | No | Directory path (default: ".") |
| `max_depth` | integer | No | Max traversal depth (default: 1) |

**Aliases:** `dir`, `directory`

**Example:**
```json
{
  "path": "src",
  "max_depth": 2
}
```

---

### `get_workspace_structure`

Get a tree view of the workspace structure.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | No | Starting path (default: ".") |
| `depth` | integer | No | Max depth (default: 2) |
| `limit` | integer | No | Max entries (default: 50, max: 200) |

**Example:**
```json
{
  "path": ".",
  "depth": 3,
  "limit": 100
}
```

> **Note:** Automatically ignores common directories like `node_modules`, `target`, `.git`, `__pycache__`, etc.

---

### `find_files`

Find files by name pattern (substring match).

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `pattern` | string | Yes | Substring to match in filenames |
| `path` | string | No | Starting path (default: workspace root) |
| `max_depth` | integer | No | Max search depth |

**Example:**
```json
{
  "pattern": "test",
  "path": "src",
  "max_depth": 5
}
```

---

### `find_files_glob` / `glob`

Find files using glob patterns.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `pattern` | string | Yes | Glob pattern (e.g., `**/*.rs`) |
| `path` | string | No | Base path for search |
| `case_sensitive` | boolean | No | Case-sensitive matching (default: false) |

**Example:**
```json
{
  "pattern": "**/*.tsx",
  "path": "src"
}
```

---

### `grep_search` / `rg`

Search file contents using regex patterns.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `pattern` | string | Yes | Regex pattern to search |
| `path` | string | No | Directory to search (default: ".") |

**Aliases for pattern:** `query`, `regex`

**Example:**
```json
{
  "pattern": "fn\\s+main",
  "path": "src"
}
```

**Returns:** Matches in format `filepath:line_number:line_content`

---

### `codebase_search`

Search codebase with context lines around matches.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `query` | string | Yes | Regex pattern to search |
| `file_pattern` | string | No | Filter files (e.g., `*.rs,*.toml`) |
| `max_results` | integer | No | Maximum results (default: 50) |

**Example:**
```json
{
  "query": "struct.*Config",
  "file_pattern": "*.rs",
  "max_results": 20
}
```

**Returns:** Matches with 2 lines of context before and after.

---

## Editor Interaction Tools

### `get_editor_state`

Get current editor context including active file, cursor position, and open files.

**Parameters:** None

**Returns:** JSON with:
- `active_file`: Currently focused file
- `open_files`: List of open file paths
- `active_tab_index`: Index of active tab
- `cursor_line`, `cursor_column`: Cursor position
- `selection_start_line`, `selection_end_line`: Selection range

---

### `open_file`

Open a file in the editor.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `path` | string | Yes | File path to open |
| `line` | integer | No | Line number to jump to |

**Example:**
```json
{
  "path": "src/main.rs",
  "line": 42
}
```

---

### `goto_line`

Navigate to a specific line in the active file.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `line` | integer | Yes | Line number (1-indexed) |
| `column` | integer | No | Column number |

**Example:**
```json
{
  "line": 100,
  "column": 15
}
```

---

### `get_selection`

Get the currently selected text in the editor.

**Parameters:** None

**Returns:** The selected text content.

---

### `replace_selection`

Replace the current selection with new content.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `content` | string | Yes | Replacement content |

**Example:**
```json
{
  "content": "new replacement text"
}
```

---

### `insert_at_cursor`

Insert content at the current cursor position.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `content` | string | Yes | Content to insert |

**Example:**
```json
{
  "content": "// TODO: implement this\n"
}
```

---

## Command Execution

### `run_command`

Execute a shell command (requires user approval).

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| `command` | string | Yes | Shell command to execute |
| `cwd` | string | Yes | Working directory |

**Example:**
```json
{
  "command": "cargo build --release",
  "cwd": "."
}
```

> **Note:** This tool requires user confirmation before execution for safety.

---

## Tool Result Handling

Tool results are automatically truncated if they exceed limits:
- **Max size:** 50KB
- **Max lines:** 2000

When truncated, the first 100 lines and last 50 lines are shown with a truncation message.

---

## Path Resolution

All paths can be:
- **Relative:** Resolved from workspace root (e.g., `src/main.rs`)
- **Absolute:** Used as-is (must be within workspace)

Paths outside the workspace are rejected for security.

---

## Adding Tools to Your AI System Prompt

To use these tools with a local AI, include the tool definitions in your system prompt. Example format:

```
You have access to the following tools:

- read_file: Read file contents. Args: {"path": "string"}
- write_file: Write to file. Args: {"path": "string", "content": "string"}
- grep_search: Search with regex. Args: {"pattern": "string", "path": "string"}
- apply_edit: Edit file. Args: {"path": "string", "old_text": "string", "new_text": "string"}
...

To use a tool, respond with:
<tool_call>
{"name": "tool_name", "arguments": {...}}
</tool_call>
```

The exact format depends on your AI provider's tool calling conventions.
