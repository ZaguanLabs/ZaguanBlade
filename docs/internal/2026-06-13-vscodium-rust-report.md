# VSCodium-Rust Inspiration Review

Date: 2026-06-13  
Reviewed source: `../inspiration/vscodium-rust`

## Executive Summary

`vscodium-rust` is a broad Tauri + React + Rust IDE prototype with a VS Code-like workbench, Monaco editor, xterm terminal, agent sidebar, checkpoints, MCP catalog, ReactFlow visualizations, background agents, and many experimental subprojects. The repo is uneven: several features are polished enough to learn from, while other areas are rough, heavily inline-styled, global-state driven, or aspirational.

The most useful frontend ideas for ZaguanBlade are not raw visuals. They are load discipline, panel isolation, per-turn agent review UX, terminal ergonomics, and active-context visualizations. Our codebase already has some comparable strengths, especially lazy editor/chat panels, CodeMirror feature gating for large files, terminal event buffering, and chat virtualization in the legacy panel. The best opportunities are targeted ports rather than a broad architectural copy.

Decision principle: adopt only high-yield ideas that enhance systems we already trust. ZaguanBlade's existing symbol index, fast context, terminal event pipeline, and typed service boundaries remain the base. Anything that requires replacing those foundations, copying unproven claims, or importing large experimental surfaces should be discarded.

Detailed Adopt implementation plan: [2026-06-13-vscodium-rust-adopt-implementation-plan.md](2026-06-13-vscodium-rust-adopt-implementation-plan.md).

## Selective Adoption Decision

### Adopt

These are worth implementing because they directly improve current ZaguanBlade systems without replacing them.

1. Idle-scheduled bootstrap for non-critical startup work.
   - Enhances our current lazy panel setup.
   - Low risk, measurable startup impact.

2. Terminal search and spawn diagnostics.
   - Enhances our existing terminal.
   - Small surface area, clear user value.

3. Per-turn multi-file agent review.
   - Enhances our uncommitted AI edit and diff infrastructure.
   - Gives users better control after multi-file agent edits.

4. Fast-context project map plus query spans.
   - Inspired by Kortex, but backed by our SQLite symbol index.
   - Should expose freshness, caps, scores, hashes, and exact line windows.

5. Active-file context panel.
   - Enhances our symbol index, ZLP, outline, graph inspector, and context sync.
   - Start text-first; graph visualization remains optional.

6. Prefix-cache-friendly context ordering.
   - Stable project overview first, volatile task/query detail second.
   - Helps prompt caching without claiming zero-token behavior.

### Maybe Later

These have potential, but only after the core enhancements above are done.

1. Curated integrations/MCP catalog.
   - Useful product UX, but needs permission and config validation work.

2. Custom agent modes and per-feature model routing.
   - Useful, but should follow chat/model settings stabilization.

3. Optional graph visualization for symbols/files/tasks.
   - Good for inspection, but should be lazy-loaded and backed by existing indexes.

4. Background agents.
   - Only after cancellation, permissions, resource limits, and result ownership are clear.

### Discard

These should not be ported.

1. Kortex as a replacement for our symbol index or fast context.
   - Our current SQLite/FTS/relationship/semantic-anchor architecture is stronger.

2. `.aim` JSON-in-binary blob as primary index storage.
   - We keep SQLite and typed local artifacts.

3. "Zero-grep" or "trust this map" prompt instructions.
   - Use fast context first, but exact files should still be verified before edits.

4. Undocumented benchmark claims.
   - No adoption without local measurement.

5. Current vector/superposition/KV-cache claims.
   - The implemented path is not a working retrieval or model-state injection system.

6. Proxy injection of large raw `.aim` text.
   - Prefix-cache ideas are useful; large opaque text injection is not.

7. Broad inline-styled UI surfaces and global `window` state patterns.
   - Rebuild concepts using our existing components, CSS tokens, hooks, and services.

## High-Value Ideas To Consider Porting

### 1. Idle-Scheduled App Bootstrap

Inspiration paths:

- `../inspiration/vscodium-rust/src/App.tsx`
- `../inspiration/vscodium-rust/src/memory_budget.ts`
- `../inspiration/vscodium-rust/src/store/index.ts`

The app shell does only critical startup work immediately, then defers search, status bar, extensions, specs, mobile, SCM, and debug initialization through `scheduleDeferredInit`, implemented with `requestIdleCallback` and a timeout fallback.

Why it matters for us:

- `src/components/Layout.tsx` already lazily imports many panels, but several hooks and listeners still initialize as part of the main layout path.
- We can move non-first-paint setup behind a small shared scheduler: optional Git/status polling, debug surfaces, history refresh, model warmup, protocol explorer listeners, and low-priority file-change notification setup.

Suggested port:

- Add a `scheduleDeferredInit` utility in `src/utils` or `src/services`.
- Use it for low-priority Tauri listeners and refreshes after the first workspace/editor/chat shell is stable.
- Keep core state recovery, active workspace restore, and visible shell layout on the immediate path.

### 2. More Aggressive Heavy-Panel Boundaries

Inspiration paths:

- `../inspiration/vscodium-rust/src/App.tsx`
- `../inspiration/vscodium-rust/src/components/Workbench.tsx`
- `../inspiration/vscodium-rust/src/components/RightSidebar.tsx`

The inspiration workbench lazy-loads nearly every heavy surface: editor, right sidebar, bottom panel, settings, MCP store, visual lab, browser surface, diff viewer, emulator panels, document outline, composer overlay, and agent review panels.

Our current state:

- `src/components/Layout.tsx` already lazy-loads explorer, chat, git, history, document viewer, settings, storage setup, and protocol explorer.
- `src/components/EditorPanel.tsx` lazy-loads CodeMirror/PDF/Markdown surfaces.
- `src/components/TerminalPane.tsx` lazy-loads `Terminal`.

Suggested port:

- Audit remaining eager imports in `Layout.tsx` and split any dev-only, modal-only, or rarely opened surface.
- Add optional preloading on hover/focus for panels that should feel instant after first interaction.
- Keep fallback UI zero-height or shell-matched so lazy boundaries do not cause layout jumps.

### 3. Per-Turn Agent Change Review

Inspiration paths:

- `../inspiration/vscodium-rust/src/components/agent/MultiFileReview.tsx`
- `../inspiration/vscodium-rust/src/components/CheckpointTimeline.tsx`
- `../inspiration/vscodium-rust/src/store/agentSlice.ts`
- `../inspiration/vscodium-rust/src-tauri/src/git_commands.rs`

The most portable UX idea is the "agent edited N files, review them" flow:

- Track files touched by the last agent turn.
- Show a banner after the turn finishes if multiple files changed.
- Open a carousel with one file at a time, unified diff, Keep/Revert actions, and a Revert All option tied to a checkpoint.
- Maintain a checkpoint timeline with file counts and expandable diff summaries.

Why it matters for us:

- We already track uncommitted AI edits and editor review transitions.
- The current review path could become more explicit and confidence-building when a turn touches several files.

Suggested port:

- Add a compact `MultiFileReview` modal using our existing diff rendering and uncommitted-change model.
- Prefer our safer backend primitives over inspiration's shell-based `git checkout HEAD~1 -- "path"` approach.
- Add "last agent turn" metadata to the chat timeline or side panel so the user can reopen review after dismissing the banner.

### 4. Terminal Search And Diagnostics

Inspiration paths:

- `../inspiration/vscodium-rust/src/terminal.ts`
- `../inspiration/vscodium-rust/src/components/terminal/TerminalInstance.tsx`
- `../inspiration/vscodium-rust/src/components/terminal/TerminalFindWidget.tsx`

Useful terminal details:

- `@xterm/addon-search` with a find widget.
- `@xterm/addon-unicode11` for better glyph handling.
- A `ResizeObserver` on each terminal pane, including inactive split panes.
- A startup diagnostic if a terminal backend spawns but no output arrives.
- Clear rationale for avoiding WebGL/canvas renderers when they blank on detached or zero-size elements.

Our current state:

- `src/components/Terminal.tsx` has robust ResizeObserver scheduling, event buffering, Linux paste/AltGr fixes, context menu actions, and WebKitGTK-safe renderer choices.
- We do not appear to expose terminal search.

Suggested port:

- Add `SearchAddon` and a small find widget to our `Terminal`.
- Consider a "no terminal output after spawn" diagnostic, but wire it to our Blade terminal events instead of polling every 50ms.
- Keep our existing `TerminalBuffer` sequencing; it is cleaner than the inspiration repo's frontend polling fallback.

### 5. Active File Context Panel

Inspiration paths:

- `../inspiration/vscodium-rust/src/components/visual/ContextSidebar.tsx`
- `../inspiration/vscodium-rust/src/components/visual/NeuralSidebarGraph.tsx`
- `../inspiration/vscodium-rust/src/components/visual/NeuralSummaryView.tsx`

The idea is a right-side "active insight" panel that changes with the active file:

- Current file path and workspace identity.
- Detected symbols.
- Related context graph.
- Historical lessons/memory snippets.

Why it matters for us:

- ZaguanBlade already has symbol indexing, ZLP, outline, graph inspector, and editor context sync.
- A compact active-context panel could make that backend work visible and useful without requiring the user to ask the agent.

Suggested port:

- Start smaller than their animated ReactFlow graph.
- Show symbols, related files, index health, last AI changes touching the file, and relevant memories/rules.
- If graph visualization is added, lazy-load it and keep it out of the initial bundle.

### 6. Visual Data/Dependency Lab

Inspiration paths:

- `../inspiration/vscodium-rust/src/components/visual/VisualLab.tsx`
- `../inspiration/vscodium-rust/pics/flow_visualizer.png`

They use ReactFlow to visualize JSON, ERD-style SQL/table data, dependency maps, and generated flow diagrams. The concept is useful for IDE workflows: "open file as graph" for structured data or project relationships.

Suggested port:

- Treat this as an optional feature, not core shell work.
- Use it first for things ZaguanBlade already understands: symbol graph, file dependency graph, git change graph, or task/agent workflow graph.
- Lazy-load the visualization library. Do not add ReactFlow to the main path.

### 7. Curated MCP Catalog

Inspiration paths:

- `../inspiration/vscodium-rust/src/mcp/mcpCatalog.ts`
- `../inspiration/vscodium-rust/src/components/McpManager.tsx`
- `../inspiration/vscodium-rust/src/components/McpStorePanel.tsx`

The curated MCP catalog is a concrete product idea: categorized entries with command, args, env fields, secret metadata, install notes, and tags. This is more user-friendly than asking users to hand-edit config.

Suggested port:

- Add a small first-party "Tool Integrations" catalog for supported local tools/providers.
- Include validation status, missing env vars, and install notes.
- Keep commands auditable before execution.

### 8. Custom Agent Modes, Rules, And Hooks

Inspiration paths:

- `../inspiration/vscodium-rust/src/store/agentSlice.ts`
- `../inspiration/vscodium-rust/src/components/RulesManager.tsx`
- `../inspiration/vscodium-rust/src/components/HooksPanel.tsx`
- `../inspiration/vscodium-rust/src/components/SteeringPanel.tsx`

The repo has UI/state for:

- User-defined modes with labels, system prompts, optional model override, and read-only flag.
- Global steering rules.
- Agent hooks triggered by events such as save/commit.
- Per-feature model routing.

Suggested port:

- We should consider this after stabilizing existing chat/model settings.
- Start with custom modes and per-feature model routing; hooks are more dangerous and need strong permission UX.

### 9. Domain-Sliced Store With Selectors

Inspiration paths:

- `../inspiration/vscodium-rust/src/store/index.ts`
- `../inspiration/vscodium-rust/src/store/layoutSlice.ts`
- `../inspiration/vscodium-rust/src/store/editorSlice.ts`
- `../inspiration/vscodium-rust/src/store/agentSlice.ts`

They use Zustand slices for editor, layout, agent, inference, settings, git, specs, extensions, terminal, security review, LSP, and debug. Components read narrow selectors, which can reduce broad re-render pressure when done carefully.

Our current state:

- We have focused contexts and hooks (`EditorContext`, `ThemeContext`, `DisplaySettingsContext`, `useChatV2`, `useTabManager`, etc.).
- Some state still flows through a very large `Layout.tsx`.

Suggested port:

- Do not rewrite state management wholesale.
- Use the pattern as a guide for extracting layout/sidebar/chat/editor state out of `Layout.tsx` in smaller increments.
- If we introduce a store, require selector-based reads and avoid dumping whole state objects into components.

## Ideas Worth Noting, But Lower Priority

### VS Code Token Map And Layout Metrics

Inspiration path: `../inspiration/vscodium-rust/src/styles.css`

They define VS Code-like CSS variables for editor/sidebar/activity/status/title/tab colors and fixed layout metrics. We already have strong theme tokens in `src/styles/theme.css` and Tailwind CSS variable integration. Still, more explicit layout tokens for title bar, status bar, tab height, sidebar width, and panel heights could make future shell work less fragile.

### Editor Model Eviction

Inspiration path: `../inspiration/vscodium-rust/src/components/Editor.tsx`

Their Monaco editor disposes inactive models when more than 12 Monaco models are open. CodeMirror has a different lifecycle, and our `EditorPanel`/buffer registry already avoids keeping a full editor instance per tab. The transferable principle is resource caps for caches, parse state, and background analysis, not the exact Monaco API.

### Inline Completion Context Window

Inspiration path: `../inspiration/vscodium-rust/src/components/editor/MonacoProviders.ts`

Their inline completion provider sends about 60 lines before the cursor, 20 after, and prepends import/use/include header lines when the cursor is deep in the file. That is a good low-cost context heuristic for any future inline completion feature.

### Background Agent Tray

Inspiration paths:

- `../inspiration/vscodium-rust/src/store/agentSlice.ts`
- `../inspiration/vscodium-rust/src/components/agentStudio/AgentManagerPanel.tsx`

The background-agent model is interesting, especially live status and result collection. It should only be ported if we first define cancellation, permissions, resource limits, and how background work competes with the active chat.

## Kortex Vs ZaguanBlade Symbol Index And Fast Context

The developer's Kortex claims are much stronger than the code in this checkout supports. There are useful ideas in Kortex, but it is not currently a superior replacement for our symbol index or fast-context implementation.

### What Kortex Claims

Kortex docs describe a `.aim` Neural VFS with:

- 1536-dimensional gist vectors.
- Holographic reduced representations and circular convolution.
- O(1) repository understanding.
- 99%+ retrieval accuracy over huge repositories.
- 99.97% prompt cache hit rate.
- "Zero-token" proxy or KV-cache injection.

The cited claims appear mostly in:

- `../inspiration/vscodium-rust/kortex/README.md`
- `../inspiration/vscodium-rust/kortex/whitepaper.md`
- `../inspiration/vscodium-rust/kortex/daemon/src/neural_math.rs`
- `../inspiration/vscodium-rust/kortex/daemon/src/gist.rs`

### What Is Actually Implemented

The IDE-integrated Kortex path is mostly a conventional text/symbol memory layer:

- `ContextIndexer` walks source files, respects a few ignore files, caps full indexing at 2,000 files, and stores at most 800 `SemanticSlot`s in memory.
- It extracts symbols for Rust, TypeScript, TSX, JavaScript, JSX, and Python with tree-sitter queries.
- Each file slot stores a path plus a short content gist, often the first 400 chars.
- `MemoryStore` persists slots, chat messages, a `symbol_graph`, project tree, and metadata into `.aim/memory.aim` as JSON plus optional binary suffix.
- Retrieval is keyword overlap against slot content/tags/category, then top-5 formatting.
- `aim_query_spans` ranks indexed symbol/path matches, then reads source files and scans lines for terms.
- `aim_pack_context` returns a compact gist, project tree summary, and relevant text previews.
- `aim-proxy` extracts strings from the `.aim` JSON header and injects up to 80KB of text into provider requests.

Key files:

- `../inspiration/vscodium-rust/src-tauri/src/context_indexer.rs`
- `../inspiration/vscodium-rust/src-tauri/src/memory_store.rs`
- `../inspiration/vscodium-rust/src-tauri/src/kortex_commands.rs`
- `../inspiration/vscodium-rust/kortex/aim-proxy/src/main.rs`

The vector pieces are not on the critical retrieval path. `daemon/src/neural_math.rs` uses naive O(N^2) circular convolution and says FFT optimization is future work. `daemon/src/gist.rs` calls convolution into `_holographic_state` and then discards it, using a simple weighted average for the actual vector update. The proxy does not inject tensors into cloud or local models; it injects text.

### Compared To Our Symbol Index

Our current symbol path is more production-shaped:

- `src-tauri/src/symbol_index/store.rs` uses SQLite tables for symbols, indexed files, symbol relationships, and semantic anchors.
- It keeps an FTS5 table for symbol name/docstring search.
- It tracks file hashes, file sizes, modified times, and index health.
- It stores byte offsets, byte lengths, ranges, parent IDs, docstrings, signatures, and content hashes.
- `src-tauri/src/symbol_index/search.rs` applies contextual boosts for active file, preferred files, and preferred directories.
- `src-tauri/src/tree_sitter/query.rs` supports Rust, TypeScript, TSX, JavaScript, JSX, Python, and Go queries.
- `src-tauri/src/context_assembly/assembler.rs` can assemble cursor-aware context with definitions, references, nearby symbols, imports, and budget allocation.

Kortex's symbol graph is a `Vec<SymbolDefinition>` protected by `RwLock` and persisted wholesale into JSON. Querying is substring matching over names and path strings. It does not have FTS, relationship tables, health snapshots, incremental per-file database replacement, semantic anchors, or our richer symbol metadata.

### Compared To Our Fast Context

Our fast context path is also more structured:

- `src-tauri/src/context_pack.rs` returns a typed `ContextPackPayload`, not just markdown strings.
- It normalizes query variants, includes workspace/open-file state, gathers primary files, related tests, related docs, local memories, enriched files, related files, impact summaries, confidence, index health, and recommended next steps.
- It can fall back cleanly when the index is unavailable.
- It includes project language summaries, directory summaries, likely entry points, and optional project index text.

Kortex has one good UX idea here: tell the model "here is the project map; read exact files on demand." But the actual Kortex implementation is mostly string packaging around a simpler index. It is not more accurate by construction, and it can become misleading because the prompt tells the model to "trust this map" even though the map can be capped, stale, truncated, or limited to short gists.

### Kortex Ideas Worth Porting

1. Add a compact whole-project symbol map mode to fast context.
   - We can generate this from SQLite: `file: kind name@line, ...`.
   - It should include freshness metadata and truncation warnings.
   - It should never claim complete knowledge if capped or stale.

2. Add a query-relevant span pack command.
   - Kortex's `aim_query_spans` concept is useful: return exact file, line window, score, hash, summary, and snippet.
   - We should build it over our symbol index plus source line windows, not over `.aim` JSON.

3. Add a prefix-cache-friendly context mode.
   - Stable project overview first, per-query variable context second.
   - This can help cloud model prompt caching without pretending to be zero-token.

4. Add indexed memory summaries as first-class local artifacts.
   - Kortex persists lessons, phase outcomes, and project memory into `.aim`.
   - We can use `.zblade/context` or local artifacts to store compact, versioned memory summaries and expose them through fast context.

5. Add "index confidence" into the prompt-facing context itself.
   - Our backend already computes health; we should make the model see whether the index is fresh, partial, stale, or fallback.

### Kortex Ideas To Avoid

1. Do not copy "zero-grep" instructions literally.
   - Telling the model not to grep/list files is unsafe if the index is stale or capped.
   - Better instruction: prefer fast context first; verify exact files when changing behavior.

2. Do not replace SQLite with a JSON-in-binary `.aim` blob.
   - It is simpler to ship, but weaker for incremental updates, queries, migrations, observability, and corruption recovery.

3. Do not accept undocumented benchmark claims.
   - I did not find benchmark code supporting the README's 99.97% cache-hit or 99% retrieval claims.

4. Do not copy the current vector implementation.
   - The HRR/convolution code is not used for retrieval in the IDE path, and the standalone daemon marks the convolution as placeholder/future optimization.

5. Do not inject 80KB of raw `.aim` header text as a proxy default.
   - That can help only when provider prefix caching behaves favorably. It still consumes prompt tokens and may degrade latency or relevance.

### Bottom Line

Kortex is a useful product direction, not a proven superior index. Its strongest practical contribution is the UX of compact project maps plus on-demand span retrieval. Our existing symbol index and fast context are a stronger base. We should port the best presentation and packaging ideas into our system, not replace our architecture.

## Things Not To Copy Directly

### Path Security Claims

Do not copy their path validation as-is.

- `../inspiration/vscodium-rust/src-tauri/src/file_commands.rs` has `is_path_valid` returning `Ok(())`.
- `../inspiration/vscodium-rust/src-tauri/src/ai_tools.rs` canonicalizes existing paths but does not reject absolute paths outside the root or traversal.
- The tests that expect traversal rejection are inside `#[cfg(any())]`, so they are disabled.

If we port any agent tool ideas, keep our own workspace boundary checks and add active tests.

### Shell-Based Revert Commands

`MultiFileReview.tsx` reverts a file by invoking `ai_execute_command` with `git checkout HEAD~1 -- "${path}"`. That is brittle and shell-sensitive. We should expose typed backend commands for file revert/checkpoint restore instead.

### Frontend Polling For Terminal Output

Their terminal polls `terminal_take_pending` every 50ms because their Tauri event stream was unreliable. Our terminal path already uses sequenced Blade events plus `TerminalBuffer`. We should not regress to polling unless we prove an event transport failure.

### Heavy Inline Styling And Global Window State

Many inspiration components use large inline style objects, `window.useStore`, global custom events, and direct DOM mutation. The concepts can be reused, but the implementation style should be adapted to our component system, CSS tokens, hooks, and typed service layer.

### Sequential Frontend File Reads In Visual Graphs

`VisualLab` builds a dependency graph by listing project files and reading up to 80 files from the frontend. That should move backend-side if we implement it, using the existing indexer/symbol graph where possible.

### Marketing-Level Feature Claims

The README and feature matrix claim a lot. Some are backed by code; others are partial, experimental, or spread across bundled subprojects. Treat claims as prompts for inspection, not as proof of mature implementation.

## Recommended Backlog For ZaguanBlade

### P0 / Small Wins

1. Add a shared `scheduleDeferredInit` utility and move low-priority boot listeners/refreshes off the immediate layout path.
2. Add terminal search with `@xterm/addon-search` and a compact find widget.
3. Add a terminal "spawned but silent" diagnostic based on Blade terminal events.
4. Add explicit shell layout tokens for title/status/tab/sidebar/panel dimensions.

### P1 / Product UX

1. Build a per-turn multi-file review modal using our uncommitted-change and diff infrastructure.
2. Add a checkpoint/restore timeline if the backend can provide safe restore points.
3. Add an active-file context panel showing symbols, related files, relevant memory/rules, and index health.
4. Add a curated integrations catalog for local tools/providers/MCP-style servers.
5. Add a fast-context "project map + query spans" mode inspired by Kortex, backed by our SQLite symbol index.

### P2 / Larger Architecture

1. Continue extracting `Layout.tsx` responsibilities into narrower hooks/stores with selector-style subscriptions.
2. Add optional graph visualization for symbol/file/task relationships, lazy-loaded and backed by backend index data.
3. Add custom agent modes and per-feature model routing.
4. Evaluate background agents only after permission, cancellation, and resource-budget semantics are clear.
5. Explore prefix-cache-friendly context ordering: stable project overview first, volatile query/task details second.

## Files Reviewed

Current project:

- `package.json`
- `src/App.tsx`
- `src/components/Layout.tsx`
- `src/components/EditorPanel.tsx`
- `src/components/CodeEditor.tsx`
- `src/components/Terminal.tsx`
- `src/components/TerminalPane.tsx`
- `src/chat/rendering/ChatViewport.tsx`
- `src/contexts/EditorContext.tsx`
- `src/index.css`

Inspiration project:

- `../inspiration/vscodium-rust/package.json`
- `../inspiration/vscodium-rust/README.md`
- `../inspiration/vscodium-rust/FEATURE_MATRIX.md`
- `../inspiration/vscodium-rust/src/App.tsx`
- `../inspiration/vscodium-rust/src/memory_budget.ts`
- `../inspiration/vscodium-rust/src/store/index.ts`
- `../inspiration/vscodium-rust/src/store/layoutSlice.ts`
- `../inspiration/vscodium-rust/src/store/agentSlice.ts`
- `../inspiration/vscodium-rust/src/components/Workbench.tsx`
- `../inspiration/vscodium-rust/src/components/RightSidebar.tsx`
- `../inspiration/vscodium-rust/src/components/Editor.tsx`
- `../inspiration/vscodium-rust/src/components/editor/MonacoProviders.ts`
- `../inspiration/vscodium-rust/src/components/terminal/TerminalInstance.tsx`
- `../inspiration/vscodium-rust/src/terminal.ts`
- `../inspiration/vscodium-rust/src/components/agent/MultiFileReview.tsx`
- `../inspiration/vscodium-rust/src/components/CheckpointTimeline.tsx`
- `../inspiration/vscodium-rust/src/components/visual/ContextSidebar.tsx`
- `../inspiration/vscodium-rust/src/components/visual/VisualLab.tsx`
- `../inspiration/vscodium-rust/src/components/visual/NeuralSidebarGraph.tsx`
- `../inspiration/vscodium-rust/src/mcp/mcpCatalog.ts`
- `../inspiration/vscodium-rust/src-tauri/src/context_indexer.rs`
- `../inspiration/vscodium-rust/src-tauri/src/memory_store.rs`
- `../inspiration/vscodium-rust/src-tauri/src/kortex_commands.rs`
- `../inspiration/vscodium-rust/src-tauri/src/context_quantizer.rs`
- `../inspiration/vscodium-rust/src-tauri/src/file_commands.rs`
- `../inspiration/vscodium-rust/src-tauri/src/ai_tools.rs`
- `../inspiration/vscodium-rust/src-tauri/src/git_commands.rs`
- `../inspiration/vscodium-rust/src-tauri/src/terminal_commands.rs`
- `../inspiration/vscodium-rust/kortex/README.md`
- `../inspiration/vscodium-rust/kortex/whitepaper.md`
- `../inspiration/vscodium-rust/kortex/aim-proxy/src/main.rs`
- `../inspiration/vscodium-rust/kortex/daemon/src/neural_math.rs`
- `../inspiration/vscodium-rust/kortex/daemon/src/gist.rs`
- `../inspiration/vscodium-rust/kortex/harness/src/lib.rs`
