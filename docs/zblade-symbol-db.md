# Zaguán Blade Symbol DB Proposal

## Summary

This document proposes the recommended symbol-index architecture for Zaguán Blade: a persistent SQLite symbol store plus a live in-memory overlay for open and dirty documents.

The goal is to give AI tools and editor features a fast, precise way to locate code symbols without repeatedly scanning the entire codebase with broad text searches.

Instead of relying on project-wide `grep_search` for common navigation tasks, the editor should maintain a structured symbol index that can answer questions such as:

- Where is `useTabManager` defined?
- Which file and line range contains `ProjectState`?
- What methods belong to a given class or impl block?
- Which symbol should be read before editing?

The system should be designed so that results stay current even while files have unsaved changes.

## Motivation

Broad repo-wide text searches are often:

- slower than necessary
- token-expensive for model-driven workflows
- imprecise when many matches exist
- a poor fit for symbol-level navigation

A symbol DB would allow the model to ask for compact, structured results first, then read only the relevant file range.

## Decision

Zaguán Blade should implement a two-layer symbol system:

1. **Persistent SQLite index** for workspace-wide durable symbol storage.
2. **Live in-memory document overlay** owned by the backend language service for open and dirty buffers.

This is the recommended V1 architecture.

It is preferable to both of the simpler alternatives:

- **grep-first retrieval** is too noisy and expensive for symbol lookup workflows.
- **snapshot-only indexing** is not fresh enough because it misses unsaved editor state.

The symbol DB must index the current editor state, not just the filesystem.

## Fit with the current codebase

This direction fits the repository well because the backend already has most of the foundation pieces:

- `LanguageService` already handles indexing and symbol queries
- `symbol_index` already provides a SQLite-backed store
- the backend already has file watching
- the frontend already has an outline surface and editor state

So the work is primarily to extend the existing indexing path with:

- stable symbol identity
- richer symbol metadata
- overlay-backed document state
- overlay-aware query APIs
- frontend document lifecycle sync

Example desired result:

```text
src/hooks/useTabManager.ts:120-210:useTabManager
```

Or, preferably, structured output:

```json
{
  "file": "src/hooks/useTabManager.ts",
  "symbol": "useTabManager",
  "kind": "function",
  "start_line": 120,
  "end_line": 210,
  "version": 42,
  "hash": "abc123"
}
```

## Goals

- Provide fast symbol lookup across the workspace.
- Return precise file ranges for targeted reads.
- Stay current while the user is typing, including unsaved changes.
- Reduce unnecessary whole-repo searches.
- Support both editor features and AI tool calls.
- Be language-aware and AST-driven rather than regex-driven.

## Non-Goals for V1

- Full-text search replacement.
- Perfect cross-reference indexing.
- Deep semantic analysis such as type inference.
- Global call graph completeness across all languages.
- Replacing existing grep tools for arbitrary text lookup.

## Core Design

The architecture should use two layers:

### 1. Persistent SQLite index

This is the durable baseline index stored on disk.

Responsibilities:

- cache symbol definitions for the project
- survive editor restart
- support fast startup and initial queries
- store metadata such as file hashes and last indexed version

### 2. Live in-memory overlay

This is the current source of truth for open or dirty files.

Responsibilities:

- reflect unsaved editor buffer changes
- update on debounced file edits
- override SQLite-backed results for dirty files
- tolerate temporary parse failures during editing

### Why both layers are needed

SQLite alone is not a good real-time source of truth for every keystroke. If indexing only occurs on save, results become stale very quickly. The overlay solves freshness; SQLite solves persistence.

Search resolution order should be:

1. check live overlay for dirty/open files
2. fall back to SQLite for clean/closed files

## Source of Truth

The symbol DB must index the editor state, not just the filesystem.

That means:

- open dirty buffers should be indexed from memory
- clean files can be served from disk-backed SQLite data
- search results should carry version metadata so downstream tools can detect staleness

Updating only on save is not sufficient.

## Architecture at a glance

### Layer 1. Persistent SQLite index

Responsibilities:

- durable workspace symbol definitions
- fast startup and restart persistence
- clean-file lookup and search
- background reindexing and cache refresh

### Layer 2. Live in-memory overlay

Responsibilities:

- current content for open files
- dirty-buffer symbol extraction
- per-document version tracking
- parse status and last-known-good fallback
- authoritative results for unsaved buffers

### Query resolution order

1. consult overlay for open or dirty files
2. fall back to SQLite for clean or closed files
3. return source and version metadata with every result

## Data Model

### Files table

Suggested fields:

- `id`
- `path`
- `language`
- `content_hash`
- `disk_mtime`
- `version`
- `is_dirty`
- `parse_status`
- `last_indexed_at`
- `source` (`disk` or `buffer` at query time, even if stored elsewhere)

### Symbols table

Suggested fields:

- `id` using a stable format such as `{file_path}::{qualified_name}#{kind}`
- `file_id`
- `name`
- `qualified_name`
- `kind`
- `start_line`
- `start_col`
- `end_line`
- `end_col`
- `byte_offset`
- `byte_length`
- `parent_symbol_id`
- `signature`
- `content_hash`
- `exported`
- `language`
- `version`

### Optional future tables

- `references`
- `imports`
- `diagnostics`
- `symbol_relationships`

## Recommended Indexes

For SQLite:

- index on `files.path`
- index on `symbols.name`
- composite index on `symbols.kind, symbols.name`
- index on `symbols.file_id`
- optional FTS index later for signatures or doc text

## Extraction Strategy

Use parser-based extraction, not regex.

Recommended options:

- reuse ZLP structure extraction where possible
- tree-sitter for incremental parsing and symbol discovery
- language-specific symbol adapters where needed

The first implementation should focus on stable symbol definitions only, such as:

- functions
- methods
- classes
- interfaces
- types
- enums
- structs
- traits
- impl blocks where relevant

## Live Update Strategy

The index must update continuously, but not by rewriting the entire DB on every keystroke.

Recommended flow:

1. file buffer changes
2. mark file as dirty
3. debounce reparse, for example `150-500ms`
4. re-extract symbols for that file only
5. update in-memory overlay immediately
6. flush to SQLite on save, idle, blur, or shutdown

This gives near-real-time freshness without excessive disk churn.

## Handling Parse Failures

Files being edited will sometimes be syntactically invalid.

The system should not discard all knowledge for that file when this happens.

Recommended behavior:

- keep the last known good symbol snapshot
- attempt reparse after debounce
- if parsing fails, mark file as `parse_error`
- continue serving last good symbols with a stale-warning flag if needed

This keeps the system useful while the user is in the middle of an incomplete edit.

## Staleness and Versioning

Line numbers alone are fragile. A symbol result should include version metadata so consumers can confirm that the result still applies.

Suggested metadata in search results:

- `id`
- `file`
- `symbol`
- `qualified_name`
- `kind`
- `start_line`
- `end_line`
- `byte_offset`
- `byte_length`
- `version`
- `hash`
- optional `parse_status`
- optional `source`

If a consumer receives a result for version `42` but the file is now version `43`, it can re-resolve the symbol before reading or editing.

## V1 API and Tooling Contract

A symbol DB becomes most useful when exposed through dedicated APIs and tools.

### `symbol_search`

Search for symbol definitions by name with optional filters.

Inputs:

- `query`
- optional `kind`
- optional `path`
- optional `language`
- optional `limit`

Returns compact structured hits such as:

```json
[
  {
    "id": "src/hooks/useTabManager.ts::useTabManager#function",
    "file": "src/hooks/useTabManager.ts",
    "symbol": "useTabManager",
    "qualified_name": "useTabManager",
    "kind": "function",
    "start_line": 120,
    "end_line": 210,
    "byte_offset": 3820,
    "byte_length": 1912,
    "version": 42,
    "source": "buffer",
    "parse_status": "clean"
  }
]
```

### `symbol_resolve`

Resolve an exact symbol in a file and return its current range and version metadata.

Use this before targeted reads or symbol-scoped edits when the caller needs to confirm the symbol is still valid for the current document version.

### `symbol_outline`

Return the symbol hierarchy for a file, such as classes with methods or impl blocks with contained functions.

This should be the backend API used by the editor outline so the UI and AI share the same symbol source of truth.

### Deferred for later phases

These are valuable, but not required for V1:

- `symbol_impact`
- `symbol_edit`
- cross-reference and caller/callee indexing

## Suggested Query Flow for Models

Instead of:

1. run broad `grep_search`
2. inspect many noisy hits
3. read large files repeatedly

Prefer:

1. `symbol_search("useTabManager")`
2. receive exact file/range hits
3. `read_file_range(...)` only for the chosen hit
4. optionally `symbol_resolve(...)` before edit if version changed

This should reduce both latency and token usage.

## Initial Scope Recommendation

### V1

- SQLite-backed file and symbol tables
- in-memory overlay for dirty/open files
- parser-based definition indexing
- stable symbol IDs and qualified names
- exact and fuzzy symbol name search
- byte-offset metadata for precise retrieval
- return file ranges plus version metadata

### V2

- parent-child symbol hierarchy
- imports/exports indexing
- symbol ranking improvements
- language-specific display names and signatures

### V3

- references
- callers/callees
- dependency graph queries
- impact analysis tooling

## Ideas Adapted from jcodemunch-mcp

This proposal overlaps conceptually with `tmp/jcodemunch-mcp`, which is a useful reference for symbol-oriented retrieval.

Important distinction:

- `jcodemunch-mcp` is primarily a repository snapshot index and retrieval system
- Zaguán Blade needs a live editor-aware symbol index that reflects unsaved buffer state

That means Zaguán Blade should borrow ideas, not architecture.

### Concepts worth adapting

#### Stable symbol IDs

A stable symbol identity such as:

```text
{file_path}::{qualified_name}#{kind}
```

is preferable to raw line-based identity because it is readable, durable across reindexing, and suitable for tool APIs.

#### Rich symbol records

A useful symbol record should include more than just a name and line range. In particular:

- `qualified_name`
- `parent_symbol_id`
- `signature`
- `byte_offset`
- `byte_length`
- `content_hash`

These support precise retrieval, hierarchy building, and staleness checks.

#### Byte-offset retrieval

Byte offsets are a strong complement to line ranges. For clean on-disk files, they allow direct retrieval of symbol content without reparsing the file. For dirty buffers, the overlay should serve equivalent content from memory.

#### Language registry pattern

A language registry similar to a `LanguageSpec` model is a good way to scale support across languages. Each language can declare:

- symbol node types
- name fields
- signature extraction rules
- doc comment strategy
- nesting/container rules

This keeps extraction logic structured and extensible.

#### Symbol hierarchy

Parent-child relationships should be stored explicitly so the system can power outline views and symbol-scoped tools.

#### Weighted search ranking

Search quality should not rely on plain substring matching alone. A weighted ranking model should boost:

- exact symbol name matches
- exact qualified name matches
- prefix matches
- signature matches
- kind, path, or language affinity

### Concepts not to port directly

#### Snapshot-first storage

`jcodemunch-mcp` stores a snapshot-style index and raw cached file content. That is suitable for repository retrieval but is not sufficient for a live editor.

Zaguán Blade should keep SQLite plus an in-memory dirty-buffer overlay.

#### JSON index storage

For Zaguán Blade, SQLite remains the better primary store because it provides better structured querying, indexing, and future extensibility.

#### AI-generated summaries in core indexing

Optional summaries may be useful later, but they should not be part of the core symbol identity or required indexing pipeline in V1.

#### Real-time assumptions

`jcodemunch-mcp` explicitly focuses on indexed retrieval, not real-time file watching. Zaguán Blade must preserve its stronger requirement: the index must remain aligned with current editor buffers.

## Tradeoffs and Risks

### Benefits

- faster symbol lookup
- lower token usage for AI workflows
- more precise targeted reads
- stronger foundation for advanced editor tools

### Risks

- implementation complexity around live updates
- parser differences across languages
- temporary inconsistency during failed parses
- higher memory use for open-buffer overlays
- cross-reference indexing may be much harder than definitions

## Recommendation

This should be implemented as a symbol index and live code-intelligence cache, not as a generic full-text database.

The architectural decision for V1 is:

> Persist clean workspace state in SQLite, but treat the backend-owned live document overlay as authoritative for open and dirty files.

If that is done well, the symbol DB becomes a strong foundation for:

- precise AI navigation
- targeted file reads
- symbol-aware editing
- a unified outline model
- future impact analysis and code intelligence features

For the concrete delivery plan, see `docs/zblade-symbol-implementation.md`.

## Proposed Next Steps

1. Extend the existing `symbol_index` schema rather than replacing it.
2. Define the final `SymbolRecord` shape with stable IDs, qualified names, parent IDs, and byte offsets.
3. Implement a live document overlay inside the backend language service.
4. Add frontend document lifecycle sync for open, change, save, and close.
5. Expose `symbol_search`, `symbol_resolve`, and `symbol_outline` as first-class APIs/tools.
6. Migrate the outline UI and AI lookup flow to prefer symbol queries before broad grep.
