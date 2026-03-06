# Zaguán Blade Symbol DB Implementation Plan

## Summary

This document turns the symbol DB proposal into a concrete implementation plan for Zaguán Blade.

The core strategy is not to build a brand-new subsystem from scratch, but to extend the language indexing foundation that already exists in the repository:

- Rust backend already has `LanguageService`
- Rust backend already has a SQLite-backed symbol store at `.zblade/index/symbols.db`
- frontend already has an `OutlinePanel` wired to ZLP structure APIs
- backend already has a filesystem watcher
- editor state already tracks active file, open files, cursor, and selection

The main missing pieces are:

- a true live dirty-buffer overlay
- version-aware symbol search and resolution tools for AI workflows
- explicit symbol identity and hierarchy improvements
- plumbing between editor buffer changes and symbol indexing
- better ranking and retrieval APIs for model usage

This plan is organized to deliver user value quickly while minimizing architectural risk.

## Current State in This Repository

### Existing backend foundations

Relevant current modules:

- `src-tauri/src/app_state.rs`
- `src-tauri/src/language_service/service.rs`
- `src-tauri/src/language_service/handler.rs`
- `src-tauri/src/fs_watcher.rs`
- `src-tauri/src/commands/tools.rs`
- `src-tauri/src/symbol_index/*` via `LanguageService`

Important observations:

1. `AppState` already initializes a SQLite symbol DB at:
   - `.zblade/index/symbols.db`
2. `LanguageService` already supports:
   - indexing one file
   - indexing a directory
   - searching symbols
   - retrieving symbols in a file
   - reindexing from in-memory content via `did_open` / `did_change`
3. `LanguageHandler` already exposes language intents like:
   - `IndexFile`
   - `IndexWorkspace`
   - `SearchSymbols`
   - `GetSymbolAt`
4. `fs_watcher.rs` already emits workspace file change events.

### Existing frontend foundations

Relevant current modules:

- `src/components/OutlinePanel.tsx`
- `src/services/zlp.ts`
- `src/contexts/EditorContext.tsx`
- editor and tab state managed through `EditorFacade` and UI state

Important observations:

1. `OutlinePanel` already wants real-time structure updates.
2. It currently reads structure from ZLP, but explicitly notes that dirty-buffer content is not yet wired through.
3. `EditorContext` already propagates cursor and selection, but not full dirty buffer contents.
4. The codebase is already conceptually aligned with a live symbol/indexing workflow.

## Desired End State

At the end of this implementation, Zaguán Blade should support:

- symbol indexing persisted in SQLite
- stable symbol IDs
- qualified symbol names and parent-child hierarchy
- live dirty-buffer symbol updates without requiring save
- file version tracking for staleness detection
- symbol search optimized for model workflows
- symbol resolution for targeted reads/edits
- file outline retrieval from the same backend symbol model
- graceful fallback when parsing temporarily fails during editing

## Implementation Principles

1. Build on the current Rust language service rather than replacing it.
2. Treat unsaved editor buffers as first-class indexed state.
3. Keep SQLite as the persistent baseline.
4. Add an in-memory overlay for live documents.
5. Prefer exact symbol retrieval over broad search.
6. Make all responses version-aware.
7. Avoid introducing cross-reference/call-graph complexity in V1.
8. Reuse tree-sitter/ZLP extraction where possible.
9. Treat AI review acceptance as a promotion point into canonical indexed state.

## Architecture Plan

## Layer 1: Persistent SQLite symbol store

This remains the durable baseline.

Responsibilities:

- workspace-wide persisted symbol definitions
- boot-time availability
- clean-file retrieval
- startup indexing and background refresh

This likely reuses the existing `symbol_index` storage module and extends it.

## Layer 2: Live in-memory document overlay

This is the new required layer.

Responsibilities:

- open file content snapshots
- dirty state tracking
- parse status tracking
- document version numbers
- last-known-good symbols for parse-failure fallback
- symbol search precedence over SQLite for dirty files

This should live inside the backend, near `LanguageService`, not only in React state.

## Layer 3: Pending AI review overlay

This is an optional but high-value layer for model-generated edits that are visible in Diff View before user acceptance.

Responsibilities:

- track proposed AI-edited content that is not yet canonical
- allow symbol extraction for review-aware UX if needed
- keep pending AI changes out of the persistent baseline until user acceptance
- support accept/reject promotion semantics
- prevent filesystem-driven DB updates from prematurely canonicalizing pending reviewed changes

This layer is conceptually distinct from normal dirty-buffer editing because the user has not yet approved the change set.

## Proposed Backend Components

### 1. `document_overlay.rs`

Create a new module under something like:

- `src-tauri/src/language_service/document_overlay.rs`

Responsibilities:

- store live document state keyed by file path
- store current content for open files
- store monotonically increasing `version`
- store `is_dirty`
- store `parse_status`
- store `last_good_symbols`
- store current extracted symbols if parse succeeds

Suggested internal model:

```rust
struct LiveDocument {
    file_path: String,
    version: i32,
    content_hash: String,
    content: String,
    is_dirty: bool,
    parse_status: ParseStatus,
    symbols: Vec<Symbol>,
    last_good_symbols: Vec<Symbol>,
    last_indexed_at: std::time::Instant,
}
```

```rust
enum ParseStatus {
    Clean,
    ParseError { message: String },
}
```

### 1b. `review_overlay.rs` or review state inside `document_overlay.rs`

If the editor already has an explicit AI Diff View with Accept/Reject semantics, represent that state explicitly rather than pretending it is identical to a normal live buffer.

Suggested model:

```rust
struct PendingReviewDocument {
    file_path: String,
    review_session_id: String,
    base_content_hash: String,
    proposed_content_hash: String,
    proposed_content: String,
    version: i32,
    parse_status: ParseStatus,
    symbols: Vec<Symbol>,
    last_good_symbols: Vec<Symbol>,
}
```

Recommended semantics:

- `Accept` promotes the reviewed content into canonical indexed state
- `Reject` drops the review overlay without updating the persistent symbol DB
- pending reviewed content should be queryable only when a caller explicitly opts into review state or when the relevant editor surface is review-aware

### 2. `symbol_query.rs`

Create a query layer responsible for merging overlay + SQLite results.

Responsibilities:

- `search_symbols`
- `resolve_symbol`
- `outline_file`
- `get_symbol_at`
- prefer overlay for dirty/open files
- otherwise query persisted symbols

This should sit above raw storage so API behavior is consistent.

### 3. `symbol_schema.rs` or storage migrations

Extend the existing symbol store schema to support:

- stable symbol IDs
- qualified names
- parent IDs
- byte offsets and lengths
- content hash
- version metadata where appropriate

If the current store already has some of these, add only what is missing.

### 4. `symbol_ranking.rs`

Encapsulate search scoring rules.

This prevents ranking logic from getting buried in command handlers.

## Schema Plan

This plan assumes the existing `symbols.db` will evolve rather than be replaced.

### Files table

Recommended fields:

- `id INTEGER PRIMARY KEY`
- `path TEXT UNIQUE NOT NULL`
- `language TEXT NOT NULL`
- `content_hash TEXT`
- `disk_mtime INTEGER`
- `last_indexed_at INTEGER`
- `symbol_count INTEGER DEFAULT 0`

### Symbols table

Recommended fields:

- `id TEXT PRIMARY KEY`
- `file_id INTEGER NOT NULL`
- `file_path TEXT NOT NULL`
- `name TEXT NOT NULL`
- `qualified_name TEXT NOT NULL`
- `kind TEXT NOT NULL`
- `parent_symbol_id TEXT NULL`
- `signature TEXT`
- `start_line INTEGER NOT NULL`
- `start_col INTEGER NOT NULL`
- `end_line INTEGER NOT NULL`
- `end_col INTEGER NOT NULL`
- `byte_offset INTEGER`
- `byte_length INTEGER`
- `content_hash TEXT`
- `exported INTEGER DEFAULT 0`
- `language TEXT NOT NULL`

### Indexes

- `CREATE INDEX idx_files_path ON files(path)`
- `CREATE INDEX idx_symbols_name ON symbols(name)`
- `CREATE INDEX idx_symbols_qname ON symbols(qualified_name)`
- `CREATE INDEX idx_symbols_kind_name ON symbols(kind, name)`
- `CREATE INDEX idx_symbols_file ON symbols(file_id)`
- optional later: FTS on `name`, `qualified_name`, `signature`

## Symbol Identity Plan

Adopt a stable symbol ID format:

```text
{file_path}::{qualified_name}#{kind}
```

Examples:

- `src/hooks/useTabManager.ts::useTabManager#function`
- `src/core/state.rs::ProjectState#struct`
- `src/core/state.rs::ProjectState.serialize#method`

Rules:

- IDs should be generated in extraction code, not query code.
- IDs should remain stable across reindexing if path, qualified name, and kind are unchanged.
- If duplicate overloads exist, append a deterministic suffix such as `~1`, `~2`.

## Live Update Flow

### Open file flow

When a file is opened in the editor:

1. frontend sends `did_open(file_path, content)` to backend
2. backend inserts/updates overlay entry
3. backend parses content
4. backend stores symbols in overlay
5. backend updates SQLite baseline only if appropriate
6. frontend outline can immediately consume overlay-backed results

### Change flow

When editor content changes:

1. frontend debounces content updates per file
2. frontend sends `did_change(file_path, version, content)`
3. backend updates overlay state
4. backend reparses only that file
5. if parse succeeds:
   - update overlay symbols
   - update `last_good_symbols`
   - mark parse status clean
6. if parse fails:
   - keep `last_good_symbols`
   - mark parse status parse_error

### Save flow

When file is saved:

1. frontend sends `did_save(file_path, version, content)` or reuse change + save signal
2. backend writes clean symbol snapshot into SQLite
3. overlay remains, but file may be marked clean
4. disk `content_hash` and `mtime` can be refreshed

### AI review flow

When a model proposes file edits through Diff View:

1. editor creates a `pending_review` state for the file
2. backend may parse the proposed content into a review overlay
3. pending review content is not treated as canonical DB state
4. normal symbol queries should default to canonical/live user-approved state unless explicitly review-aware
5. on `Accept`:
   - reviewed content is promoted into canonical state
   - SQLite is updated
   - review overlay is cleared
6. on `Reject`:
   - review overlay is discarded
   - SQLite is not updated

This makes AI review acceptance a high-confidence synchronization point without replacing normal dirty-buffer syncing.

### Close flow

When file is closed:

1. if file is clean, overlay can be evicted
2. if file is dirty, decide policy:
   - keep until saved or app shutdown, or
   - evict with warning if unsaved buffers are not retained backend-side

Recommendation for V1:
- keep open dirty buffers in overlay until tab close or workspace unload
- if closed unsaved content cannot be retained reliably, clearly define that the overlay is authoritative only while the document remains open

## Frontend Integration Plan

## Goal

Wire actual editor content into the backend indexer.

## Required frontend work

### 1. Introduce document sync hooks

Add a service or hook that publishes document lifecycle events:

- document opened
- document changed
- document saved
- document closed

Likely locations:

- `src/components/CodeEditor.tsx`
- editor state hooks under `src/hooks/`
- or a new `src/services/documentSync.ts`

### 2. Extend `EditorFacade` or create `DocumentFacade`

`EditorFacade` currently syncs cursor and selection, but not content.

Options:

- extend `EditorFacade` with document methods, or
- create a dedicated `DocumentFacade`

Recommended API:

- `didOpen(filePath, content)`
- `didChange(filePath, version, content)`
- `didSave(filePath, version, content)`
- `didClose(filePath)`
- `didStartReview(filePath, reviewSessionId, baseContent, proposedContent)`
- `didAcceptReview(filePath, reviewSessionId, content)`
- `didRejectReview(filePath, reviewSessionId)`

### 3. Update `OutlinePanel`

`OutlinePanel.tsx` already contains a note that dirty content should be piped later.

Change it so it does not rely on stale disk content assumptions.

Preferred backend-facing behavior:

- ask backend for `symbol_outline(file)`
- backend serves overlay-backed results if file is dirty
- frontend does not need to manually inject content into every outline query once sync exists

This is cleaner than always pushing full content through every outline request.

## Backend API Plan

Add explicit commands or language intents for symbol DB operations.

## New commands / intents

### Document sync

Add language intents or Tauri commands for:

- `DocumentDidOpen`
- `DocumentDidChange`
- `DocumentDidSave`
- `DocumentDidClose`

If using the blade protocol, keep these inside `LanguageIntent`.

### Symbol search

Add a tool/API operation equivalent to:

- `symbol_search(query, kind?, path?, language?, limit?)`

Returns:

- `id`
- `file`
- `name`
- `qualified_name`
- `kind`
- `start_line`
- `end_line`
- `byte_offset`
- `byte_length`
- `version`
- `source` (`buffer`, `disk`, or `pending_review`)
- `parse_status`

### Symbol resolve

Add:

- `symbol_resolve(file, symbol_id | qualified_name)`

Used before:

- `read_file_range`
- symbol-aware edits
- future `symbol_edit`

### Symbol outline

Add:

- `symbol_outline(file)`

Returns hierarchical symbols for the file.

### Symbol at cursor

The current `get_symbol_at` is already close to this.
Extend it to consult overlay first.

## Search Ranking Plan

Create weighted ranking rules for model-facing symbol search.

### V1 scoring

- exact `name` match: highest boost
- exact `qualified_name` match: highest boost
- prefix match on `name`
- substring match on `name`
- substring match on `qualified_name`
- boost for matching `kind`
- boost for active/open file
- optional boost for path affinity
- by default, demote or exclude `pending_review` results unless the caller explicitly requests them

### V1 filters

- `kind`
- `language`
- `path prefix`
- `limit`

### Why this matters

This is what makes symbol search a true replacement for broad grep in many workflows.

## Outline Unification Plan

Today, `OutlinePanel` uses ZLP structure.
Long term, outline and symbol DB should share the same extraction pipeline.

### Recommended direction

- keep ZLP structure endpoint for now if needed
- internally route it through the same extraction/indexing path used by the symbol DB
- avoid maintaining two different symbol extraction systems if possible

This reduces drift between:

- what the editor outline shows
- what AI tools can search
- what symbol resolution returns

## Parsing and Failure Strategy

### Requirements

- parsing must be tolerant while the user is typing
- parse failures must not wipe out the useful symbol state

### Policy

For each live document:

- keep `symbols`
- keep `last_good_symbols`
- keep `parse_status`

If parsing fails:

- return `last_good_symbols`
- mark result as stale or parse-error-backed
- surface parse status to callers

This ensures tools remain useful during incomplete edits.

## File Watching Plan

The existing `fs_watcher.rs` should continue to handle external changes on disk.

Use it for:

- files modified outside the editor
- newly created files
- deleted files
- renamed files

Recommended backend behavior on file watcher events:

1. if file is not dirty in overlay and not in pending review:
   - reindex from disk
2. if file is dirty in overlay:
   - do not blindly overwrite overlay state
   - mark external-drift conflict if needed
3. if file is in pending review:
   - do not treat watcher-triggered disk writes as canonical until review state is resolved
   - suppress or defer DB promotion while review is active

V1 simplification:
- overlay wins for open dirty buffers
- watcher updates SQLite only for clean files
- pending review state blocks automatic canonicalization from watcher events

## Detailed Delivery Phases

## Phase 0: Discovery and schema alignment

Goal:
- understand current `symbol_index` schema and extend safely

Tasks:

1. audit `src-tauri/src/symbol_index/*`
2. document current tables and queries
3. map current `tree_sitter::Symbol` fields to desired fields
4. design migration path for stable IDs and qualified names
5. confirm whether byte offsets and parent relationships already exist

Deliverables:

- schema diff
- migration plan
- list of missing fields

## Phase 1: Stable symbol model

Goal:
- make persisted symbols rich and stable enough for future tools

Tasks:

1. extend symbol extraction to produce:
   - stable `id`
   - `qualified_name`
   - `parent_symbol_id`
   - `content_hash`
   - `byte_offset` / `byte_length`
2. update SQLite schema and inserts
3. update search results to include richer metadata
4. add tests for ID stability and duplicate disambiguation

Deliverables:

- enriched symbol records in SQLite
- schema migration applied
- tests passing

## Phase 2: Live document overlay

Goal:
- support unsaved buffer indexing

Tasks:

1. implement `document_overlay.rs`
2. integrate overlay into `LanguageService`
3. modify `did_open`, `did_change`, `did_close` to use overlay rather than only file cache
4. track per-document version
5. keep `last_good_symbols` on parse failure
6. mark parse status in results

Deliverables:

- live open-buffer symbol state in backend
- overlay-preferred lookup path
- tests for dirty-buffer behavior

## Phase 3: Frontend document sync

Goal:
- feed editor content to backend in real time

Tasks:

1. identify actual editor content source in `CodeEditor` or related hooks
2. send `did_open` on file open
3. debounce `did_change` on content edits
4. send `did_save` on save
5. send `did_close` on close
6. ensure multiple open tabs are handled independently

Debounce recommendation:

- 150ms to 300ms for content sync
- skip duplicate payloads by content hash if possible

Deliverables:

- live backend synchronization from UI editors
- no requirement to save before outline/search updates

## Phase 3b: Review-gated AI indexing

Goal:
- prevent proposed AI edits from becoming canonical indexed state before user approval

Tasks:

1. model Diff View edits as `pending_review` state
2. add accept/reject review lifecycle events
3. parse pending review content separately from canonical content
4. suppress filesystem-driven canonical reindex during active review
5. decide whether review-aware surfaces can opt into `pending_review` query results
6. promote symbols to SQLite only on `Accept`

Deliverables:

- explicit review overlay or equivalent state model
- accept/reject-driven DB promotion
- no premature indexing of rejected AI changes

## Phase 4: Overlay-aware symbol queries

Goal:
- expose useful symbol retrieval APIs for AI and UI

Tasks:

1. implement merged search path: overlay first, SQLite second
2. add `symbol_search`
3. add `symbol_resolve`
4. add `symbol_outline`
5. make existing `get_symbol_at` overlay-aware
6. include version/source/parse metadata in responses

Deliverables:

- model-usable symbol query APIs
- UI-usable file outline API

## Phase 5: Tool integration

Goal:
- let AI workflows prefer symbol search over broad grep

Tasks:

1. add new tool definitions for symbol APIs
2. update prompting/tool docs to prefer symbol search when appropriate
3. expose compact structured responses
4. preserve `grep_search` for text, comments, literals, and fallback use cases

Deliverables:

- `symbol_search` tool
- `symbol_resolve` tool
- `symbol_outline` tool
- updated tool guidance

## Phase 6: UI adoption

Goal:
- use the new system in existing editor surfaces

Tasks:

1. update `OutlinePanel` to consume `symbol_outline`
2. optionally add “Go to symbol” UI powered by `symbol_search`
3. optionally add symbol breadcrumbs later
4. show parse-state badge if outline is based on stale last-good symbols

Deliverables:

- live-updating outline backed by the new symbol system

## Phase 7: Performance hardening

Goal:
- make it scale to real workspaces

Tasks:

1. measure parse latency by language and file size
2. add limits for huge files
3. add coalescing for rapid edits
4. avoid SQLite writes on every keystroke
5. flush persisted snapshots only on save/idle/close
6. benchmark search latency

Success thresholds:

- symbol search under 30ms for common queries on indexed repos
- file reparse under 100ms for typical source files
- outline refresh feels immediate on normal editing

## Concrete File-by-File Plan

## Backend

### `src-tauri/src/app_state.rs`

Changes:

- add overlay manager into app state if not encapsulated entirely inside `LanguageService`
- add review-state coordination if AI Diff View state is managed centrally
- ensure lifecycle initialization happens with workspace root and DB path

### `src-tauri/src/language_service/service.rs`

Changes:

- replace simple `file_cache` role with richer live document overlay
- add optional review overlay / pending review state
- add overlay-aware search and resolve methods
- add explicit save and review-accept handling
- ensure dirty/open docs override persisted state while pending review docs do not become canonical until accepted

### `src-tauri/src/language_service/handler.rs`

Changes:

- add new language intents or commands for document sync
- add review accept/reject intents if using Diff View integration
- add new symbol query responses
- make existing symbol lookup consult overlay-aware service methods

### `src-tauri/src/fs_watcher.rs`

Changes:

- ignore or defer disk updates for dirty overlay-backed files
- suppress canonical reindex for files with active pending review state
- trigger background reindex for clean files on external modification

### `src-tauri/src/symbol_index/*`

Changes:

- extend schema
- add migrations
- add query helpers for resolve and outline
- add indexes for name, qualified name, kind

### `src-tauri/src/tree_sitter/*`

Changes:

- extend symbol extraction for stable IDs and hierarchy
- add byte offset and content hash if missing

## Frontend

### `src/components/CodeEditor.tsx`

Changes:

- detect document open/change/save/close lifecycle
- integrate Diff View review lifecycle if AI edits are review-gated
- send debounced document sync messages

### `src/services/editorFacade.ts`

Changes:

- optionally add document lifecycle APIs
- add review accept/reject lifecycle APIs if housed here
- or create a dedicated `DocumentFacade`

### `src/components/OutlinePanel.tsx`

Changes:

- switch from raw `ZLPService.getStructure(file, "")` assumptions
- call new backend outline API
- optionally show stale/parse-error state

### `src/services/zlp.ts` or new symbol service

Changes:

- add frontend wrappers for symbol search / resolve / outline
- keep response shapes explicit and typed

### `src/types/zlp.ts` or new symbol types file

Changes:

- add types for:
  - `SymbolSearchResult`
  - `SymbolResolveResult`
  - `SymbolOutlineNode`
  - parse status / source / version metadata

## API Shape Recommendations

## `symbol_search`

Request:

```json
{
  "query": "useTabManager",
  "kind": "function",
  "path": "src/hooks",
  "language": "typescript",
  "limit": 10
}
```

Response:

```json
{
  "results": [
    {
      "id": "src/hooks/useTabManager.ts::useTabManager#function",
      "file": "src/hooks/useTabManager.ts",
      "name": "useTabManager",
      "qualified_name": "useTabManager",
      "kind": "function",
      "start_line": 120,
      "end_line": 210,
      "byte_offset": 3820,
      "byte_length": 1912,
      "version": 7,
      "source": "buffer",
      "parse_status": "clean"
    }
  ]
}
```

## `symbol_resolve`

Request:

```json
{
  "file": "src/hooks/useTabManager.ts",
  "symbol": "src/hooks/useTabManager.ts::useTabManager#function"
}
```

Response:

```json
{
  "found": true,
  "id": "src/hooks/useTabManager.ts::useTabManager#function",
  "file": "src/hooks/useTabManager.ts",
  "name": "useTabManager",
  "qualified_name": "useTabManager",
  "kind": "function",
  "start_line": 120,
  "end_line": 210,
  "version": 7,
  "source": "buffer",
  "parse_status": "clean"
}
```

## `symbol_outline`

Request:

```json
{
  "file": "src/hooks/useTabManager.ts"
}
```

Response:

```json
{
  "file": "src/hooks/useTabManager.ts",
  "version": 7,
  "source": "buffer",
  "parse_status": "clean",
  "nodes": [
    {
      "id": "src/hooks/useTabManager.ts::useTabManager#function",
      "name": "useTabManager",
      "qualified_name": "useTabManager",
      "kind": "function",
      "start_line": 120,
      "end_line": 210,
      "children": []
    }
  ]
}
```

## Testing Plan

## Backend unit tests

Add tests for:

1. stable symbol ID generation
2. duplicate overload disambiguation
3. byte offset correctness
4. parent-child hierarchy extraction
5. dirty-buffer overlay precedence over SQLite
6. parse-error fallback to last-good symbols
7. file save flush to SQLite
8. pending review content does not update canonical DB until accept
9. reject drops review symbols without DB promotion
10. external file watcher updates for clean files

## Backend integration tests

Add tests for:

1. index workspace, then search symbols
2. open file with unsaved changes, then search updated symbol name
3. outline reflects dirty content without save
4. resolve result version changes after edit
5. pending AI diff does not affect canonical search results before accept
6. accepted AI diff updates canonical search results after promotion

## Frontend tests

If adjacent test patterns exist, add tests for:

1. debounced document sync behavior
2. outline refresh after local edits
3. no excessive request storm during typing

## Rollout Strategy

## Stage 1

Ship backend-only improvements first:

- stable IDs
- richer symbol schema
- overlay manager
- search/resolve/outline APIs

Use existing UI only lightly.

## Stage 2

Connect active editor content sync.

This is when the system becomes truly valuable.

## Stage 3

Migrate `OutlinePanel` and AI tools to use the new symbol APIs by default.

## Stage 4

Add optional UX enhancements:

- Go to symbol palette
- breadcrumbs
- symbol-aware edit helpers

## Risks and Mitigations

### Risk: duplicate symbol extraction systems

Mitigation:
- converge outline and AI retrieval onto one extraction backend

### Risk: too many writes during typing

Mitigation:
- keep dirty state in overlay
- persist only on save/idle/close
- for AI review flows, persist canonical symbols only on accept

### Risk: parser failures make outline useless

Mitigation:
- keep last-good symbol snapshot
- return parse status explicitly

### Risk: version drift breaks downstream edits

Mitigation:
- include file version in all symbol results
- require re-resolve before symbol edit when version changed

### Risk: schema migration complexity

Mitigation:
- do Phase 0 first and evolve current symbol store incrementally

## Recommended Immediate Next Actions

1. Audit `src-tauri/src/symbol_index/*` and document the current schema.
2. Define the final Rust `SymbolRecord` shape for persisted and overlay-backed symbols.
3. Implement the live document overlay in the language service.
4. Add document lifecycle sync from the editor frontend.
5. Decide whether AI Diff View participates through explicit `pending_review` lifecycle events.
6. Expose `symbol_search`, `symbol_resolve`, and `symbol_outline`.
7. Migrate `OutlinePanel` to the new outline API.

## Definition of Done for V1

V1 is complete when all of the following are true:

- symbol records have stable IDs and qualified names
- SQLite stores rich symbol metadata
- open dirty buffers are indexed in backend memory
- pending AI review changes do not become canonical indexed state until accepted
- symbol search returns overlay-backed current results
- file outline updates without saving the file
- parse failures preserve last-good symbols
- model-facing symbol search exists as a dedicated API/tool
- broad grep is no longer the default first step for symbol lookup workflows
