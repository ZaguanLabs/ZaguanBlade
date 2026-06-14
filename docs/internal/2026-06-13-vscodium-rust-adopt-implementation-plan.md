# VSCodium-Rust Adopt Implementation Plan

Date: 2026-06-13  
Source review: `docs/internal/2026-06-13-vscodium-rust-report.md`  
Inspiration source: `../inspiration/vscodium-rust`

## Purpose

This plan turns the six items in the report's `Adopt` section into an implementation roadmap for ZaguanBlade.

The adoption rule is strict: port the smallest high-yield behavior that strengthens systems we already trust. Do not port architecture that replaces our symbol index, fast context, terminal event pipeline, editor model, or typed backend boundaries.

## Adopt Scope

We should implement these six items:

1. Idle-scheduled bootstrap for non-critical startup work.
2. Terminal search and spawn diagnostics.
3. Per-turn multi-file agent review.
4. Fast-context project map plus query spans.
5. Active-file context panel.
6. Prefix-cache-friendly context ordering.

Everything else from the inspiration repo is out of scope for this plan unless one of these items needs a small supporting utility.

## Release Priority Filter

`v0.8.2` has shipped. The small enhancement items that were previously nice-to-have for `v0.8.2` are now targeted at `v0.8.3` because they enhance existing systems without replacing architecture.

### Completed For v0.8.3

These items are implemented and pushed in commit `f89410b`.

1. Deferred startup utility with one low-risk caller.
   - What went in: `scheduleDeferredInit` plus tests, with the `file-changes-detected` Tauri listener registered after idle scheduling.
   - Why it enhances us: it starts startup-load discipline while keeping notification initialization immediate and leaving workspace/editor/chat restoration untouched.
   - Verification: `bun test src/**/*.test.ts` and `bun run build` passed.

2. Terminal search.
   - Why: high user value and expected IDE ergonomics.
   - What went in: `@xterm/addon-search`, a compact find widget, scoped `Ctrl/Cmd+F`, next/previous, close, search options, and single-line selection prefill.
   - Why it enhances us: it adds a missing ergonomic layer to an already strong terminal implementation.
   - Verification: terminal output, resize, paste/copy, and Blade event paths were left intact; `bun test src/**/*.test.ts` and `bun run build` passed.

3. Terminal spawn diagnostics.
   - Why: helps diagnose terminals that start but produce no output.
   - What went in: Blade-event-based tracking of `Spawned`, first `Output`, and `Exit`, with mild local status lines after timeouts.
   - Why it enhances us: it improves observability without changing our event pipeline.
   - Verification: no frontend polling was added; timers clear on first output, exit, or unmount.

### Remaining v0.8.3 Filter

Only critical regressions from the completed enhancement set should remain in `v0.8.3`. Do not pull larger Kortex/context changes into `v0.8.3` unless they become required to fix a release-blocking defect.

### v0.9.0 Candidates

These are larger product or architecture changes. They should be planned for `v0.9.0` instead of being squeezed into `v0.8.3`.

1. Per-turn multi-file agent review.
   - Why: highest product-trust win after broad AI edits.
   - What goes in: turn-level grouping, review banner, per-file diff modal, keep/revert actions, group keep/revert, stale detection, and id-based backend commands.
   - Why it enhances us: it builds on our existing uncommitted-change tracker and editor review transitions instead of replacing them.
   - Why not `v0.8.3`: incorrect grouping or revert behavior can affect user files, so it deserves careful tests and rollout.

2. Fast-context project map plus query spans.
   - Why: highest-yield Kortex-inspired backend idea.
   - What goes in: typed project map, ranked exact spans, scores, reasons, hashes, freshness, caps, warnings, and request flags.
   - Why it enhances us: it packages our stronger SQLite symbol index into a compact model-facing overview plus exact evidence.
   - Why not `v0.8.3`: it touches core context behavior and must be benchmarked/tested against stale and capped indexes.

3. Prefix-cache-friendly context ordering.
   - Why: improves deterministic prompt assembly and may improve provider prompt caching.
   - What goes in: stable/volatile context sections, section fingerprints, section budgets, debug metadata, and guarded prompt wording.
   - Why it enhances us: it makes our existing fast context easier to cache, inspect, and reason about.
   - Why not `v0.8.3`: prompt ordering changes model behavior and needs flagged comparison before becoming default.

4. Active-file context panel.
   - Why: makes symbol/index/context intelligence visible before the user asks the agent.
   - What goes in: lazy text-first panel showing file identity, symbols, current symbol, related files, memories/rules, and index health.
   - Why it enhances us: it turns hidden backend context into an inspectable workbench surface.
   - Why not `v0.8.3`: it needs backend payload discipline and frontend placement decisions; graph visualization is explicitly not part of the first pass.

Release recommendation: keep `v0.8.3` focused on the completed enhancement set and regression fixes. Make `v0.9.0` the agent review plus smarter fast-context release.

## Current Baseline

ZaguanBlade already has stronger foundations than the inspiration repo in several important areas:

- Frontend panels are already partially lazy-loaded: `Layout.tsx`, `EditorPanel.tsx`, `TerminalPane.tsx`, and chat surfaces do meaningful bundle splitting.
- Terminal output already uses sequenced Blade events plus `TerminalBuffer`; it should not regress to frontend polling.
- AI edits are already tracked as uncommitted changes with accept/reject commands and editor review transitions.
- Fast context already returns typed `ContextPackPayload` data instead of opaque markdown.
- Symbol search already uses SQLite, FTS5, relationships, semantic anchors, file freshness metadata, and contextual boosts.
- Backend code already exposes index health and project context through typed services.

The inspiration repo is useful where it shows ergonomic product surfaces around these capabilities. It is not a better backend architecture to copy wholesale.

## Implementation Order

Use this order to get value early while de-risking larger context work.

### Phase 0: Measurement And Flags

Do this first.

- Add lightweight debug/perf labels for deferred startup work.
- Add feature flags for new UI surfaces so they can ship dark or disabled by default:
  - `deferredBootstrap`
  - `terminalFind`
  - `terminalSpawnDiagnostics`
  - `agentTurnReview`
  - `contextProjectMap`
  - `activeContextPanel`
  - `prefixCacheContextOrdering`
- Prefer existing frontend debug flag patterns and backend feature flag conventions over creating a second feature flag system.

Exit criteria:

- A developer can enable each feature independently.
- Existing shell, editor, terminal, and chat behavior is unchanged when flags are off.

### Phase 1: Small Frontend Wins

Implement:

- Idle-scheduled bootstrap.
- Terminal search.
- Terminal spawn diagnostics.

These are low-risk and should produce immediate UX wins.

### Phase 2: Review UX

Implement:

- Per-turn multi-file agent review.

This uses existing uncommitted-change infrastructure and improves control after agent edits.

### Phase 3: Context Backend Enhancements

Implement:

- Fast-context project map.
- Query-relevant spans.
- Prefix-cache-friendly context ordering primitives.

This is the core Kortex-inspired work, but built on our SQLite symbol store and typed context pack.

### Phase 4: Context Frontend Surface

Implement:

- Active-file context panel.

This should consume the improved backend context data. Start text-first; graph visualization is not part of the initial implementation.

## 1. Idle-Scheduled Bootstrap

### What Goes In

Add a small frontend scheduler for low-priority startup work:

- `src/utils/deferredInit.ts`
- `src/utils/deferredInit.test.ts`

Proposed API:

```ts
export type DeferredInitPriority = 'idle' | 'soon' | 'background';

export interface DeferredInitOptions {
  label: string;
  priority?: DeferredInitPriority;
  timeoutMs?: number;
  signal?: AbortSignal;
}

export interface DeferredInitHandle {
  cancel(): void;
}

export function scheduleDeferredInit(
  task: () => void | Promise<void>,
  options: DeferredInitOptions,
): DeferredInitHandle;
```

Behavior:

- Use `requestIdleCallback` when available.
- Fall back to `window.setTimeout`.
- Always apply a maximum timeout so important background listeners are eventually registered.
- Catch errors and log them with the task label.
- Record debug perf markers with existing `recordDebugPerf`.
- Support cancellation for effects that unmount before the task runs.

Initial priority mapping:

```ts
const PRIORITY_TIMEOUT_MS = {
  soon: 150,
  idle: 500,
  background: 1500,
};
```

### Where It Goes

Primary targets:

- `src/App.tsx`
- `src/components/Layout.tsx`
- `src/utils/debugPerf.ts`

Do not move core first-paint work:

- Theme initialization.
- Active workspace restore needed to render the shell.
- Editor/chat state required for the visible route.
- Anything that prevents data loss on shutdown or immediate save.

Move or gate only non-critical startup work:

- File-change notification listener setup in `App.tsx`.
- Debug-only listeners and debug surface probes.
- Optional warmup that is not needed for first paint.
- Low-priority status refreshes.
- Project-settings change listener registration if the workspace is not yet interactive.
- History refresh or secondary project-state persistence probes that can run after the shell is stable.

### Why This Enhances Current Behavior

`Layout.tsx` already lazy-loads large panels, but the app still performs several listeners, effects, and refresh paths during initial mount. Deferring non-critical work should reduce main-thread contention and make the shell interactive sooner.

The important distinction from the inspiration repo: we keep our existing lazy boundaries and only add an orchestration utility. We are not adopting their global store or broad startup structure.

### Implementation Steps

1. Create `src/utils/deferredInit.ts`.
2. Add tests for:
   - Immediate cancellation before execution.
   - Fallback scheduling when `requestIdleCallback` is absent.
   - Error logging does not throw through React effects.
   - Default timeout selection by priority.
3. Wrap `App.tsx` file-change notification listener setup:
   - Schedule listener registration after idle.
   - Keep unlisten cleanup in the effect.
   - Ensure notifications still appear after initial startup.
4. Move one low-risk `Layout.tsx` listener or refresh behind the scheduler.
5. Add debug perf labels:
   - `deferredInit.schedule.<label>`
   - `deferredInit.run.<label>`
   - `deferredInit.done.<label>`
   - `deferredInit.error.<label>`
6. Verify no first-paint state becomes unavailable.
7. Expand usage to the remaining low-priority targets after the first commit is stable.

### Acceptance Criteria

- `bun test src/utils/deferredInit.test.ts` passes.
- `bun run build` passes.
- The app opens to an interactive shell with the same restored workspace/editor state as before.
- File-change notifications still work after startup.
- Debug perf output shows deferred tasks running after initial layout render.
- No visible panel jumps are introduced.

### Risks And Mitigations

- Risk: deferring a listener can miss an early event.
  - Mitigation: only defer listeners where missed early events are harmless or recoverable by refresh.
- Risk: background work still runs in a burst.
  - Mitigation: stagger timeouts by priority and label.
- Risk: React effect cleanup races scheduled async work.
  - Mitigation: require `AbortSignal` checks before starting and after awaited work.

## 2. Terminal Search And Spawn Diagnostics

### What Goes In

Add terminal find support using Xterm's search addon:

- Dependency: `@xterm/addon-search` compatible with the current `@xterm/xterm` major line.
- New component: `src/components/terminal/TerminalFindWidget.tsx`
- Optional pure state helper: `src/components/terminal/terminalFindState.ts`
- Tests for pure find-widget state if helper is added.

Core behavior:

- `Ctrl+F` or `Cmd+F` opens the find widget when terminal focus is inside the terminal.
- `Escape` closes the widget and returns focus to the terminal.
- `Enter` moves to next match.
- `Shift+Enter` moves to previous match.
- Buttons for next, previous, close, case-sensitive, whole-word, and regex.
- Query input should not resize the terminal.
- Match count is optional for the first pass because Xterm search does not require it for value.

Add spawn diagnostics:

- Track `Terminal.Spawned`, first `Terminal.Output`, and `Terminal.Exit` events for each terminal id.
- If a spawned terminal produces no output after a short delay, show a small terminal-local diagnostic.
- Use our Blade events and `TerminalBuffer`. Do not poll the backend.

Suggested diagnostic thresholds:

- `2500ms`: write a muted diagnostic line to the terminal if no output has arrived.
- `8000ms`: show a stronger inline status if still silent and not exited.
- Skip or soften diagnostics for `externalProcess=true`, because externally managed commands may intentionally stay quiet.

### Where It Goes

Primary targets:

- `package.json`
- `src/components/Terminal.tsx`
- `src/components/TerminalPane.tsx`
- `src/components/terminal/TerminalFindWidget.tsx`
- `src/styles` or existing terminal CSS location
- `public/locales/en/translation.json`
- `public/locales/es/translation.json`

Potential i18n keys:

```json
{
  "terminal.find.placeholder": "Find in terminal",
  "terminal.find.previous": "Previous match",
  "terminal.find.next": "Next match",
  "terminal.find.matchCase": "Match case",
  "terminal.find.wholeWord": "Whole word",
  "terminal.find.regex": "Use regular expression",
  "terminal.find.close": "Close find",
  "terminal.diagnostics.noOutput": "Terminal started, waiting for output..."
}
```

### Why This Enhances Current Behavior

Our terminal is already stronger than the inspiration implementation in event ordering, resize handling, paste/AltGr behavior, and WebKitGTK-safe rendering. The missing ergonomic feature is terminal search, which users expect in any serious IDE terminal.

Spawn diagnostics add observability without changing the backend path. They help distinguish "command is still starting" from "terminal event stream or process spawn is broken."

### Implementation Steps

1. Add `@xterm/addon-search`.
2. Import and load `SearchAddon` in `Terminal.tsx` next to `FitAddon` and `WebLinksAddon`.
3. Store refs:
   - `searchAddonRef`
   - `findVisibleRef` or React state.
   - `hasOutputRef`.
   - `spawnDiagnosticTimersRef`.
4. Add keyboard handling:
   - If terminal has focus and `Ctrl/Cmd+F`, prevent default and open find.
   - When find input is focused, keep normal text editing behavior.
5. Implement `TerminalFindWidget`.
6. Wire widget actions:
   - `findNext(query, options)`
   - `findPrevious(query, options)`
   - Clear decorations when query is empty or widget closes, if addon supports it in the installed version.
7. Add spawn diagnostic tracking:
   - On `Terminal.Spawned` for matching id, set `hasOutput=false` and arm timers.
   - On first `Terminal.Output`, set `hasOutput=true` and clear timers.
   - On `Terminal.Exit`, clear timers.
   - On component unmount, clear timers and dispose search addon.
8. Style the widget as a compact overlay attached to the terminal top-right or top bar.
9. Add i18n keys.
10. Run build and manual terminal smoke tests.

### Acceptance Criteria

- `Ctrl/Cmd+F` opens terminal search without triggering browser/page find.
- Search finds visible and scrollback terminal text.
- Next/previous works with keyboard and buttons.
- Closing search returns focus to the terminal.
- Terminal copy/paste/context menu behavior remains unchanged.
- Terminal resize remains stable when find opens and closes.
- Silent-spawn diagnostic appears only for a terminal that spawned and produced no output within the threshold.
- No frontend polling loop is introduced.

### Risks And Mitigations

- Risk: Search addon version mismatch with Xterm 6.
  - Mitigation: install the addon version resolved by the package manager for the current Xterm line and validate build types.
- Risk: `Ctrl+F` conflicts with app-level command handling.
  - Mitigation: scope interception to terminal focus or terminal container.
- Risk: Diagnostics create noisy output for quiet commands.
  - Mitigation: use a mild status first, skip or soften for external processes, and clear on first output.

## 3. Per-Turn Multi-File Agent Review

### What Goes In

Add a review flow that groups file edits by the agent turn that produced them.

New frontend pieces:

- `src/components/agent-review/AgentTurnReviewBanner.tsx`
- `src/components/agent-review/AgentTurnReviewModal.tsx`
- `src/components/agent-review/AgentTurnReviewFileList.tsx`
- `src/components/agent-review/AgentTurnDiffView.tsx`
- `src/utils/agentTurnReviewState.ts`
- `src/utils/agentTurnReviewState.test.ts`

Potential shared types:

```ts
export interface AgentTurnChangeSet {
  id: string;
  conversationId: string | null;
  messageId: string | null;
  turnId: string | null;
  startedAtMs: number;
  completedAtMs: number | null;
  status: 'open' | 'partially-reviewed' | 'reviewed' | 'stale';
  files: AgentTurnReviewFile[];
}

export interface AgentTurnReviewFile {
  changeId: string;
  path: string;
  status: 'pending' | 'kept' | 'reverted' | 'stale' | 'failed';
  additions: number;
  deletions: number;
  unifiedDiff: string;
  snapshotId: string | null;
  fileModifiedMs: number | null;
  conflictReason?: string;
}
```

Backend support should be minimal at first because we already have uncommitted-change commands:

- Reuse:
  - `get_uncommitted_changes`
  - `accept_change`
  - `accept_file_changes`
  - `accept_all_changes`
  - `reject_change`
  - `reject_file_changes`
  - `reject_all_changes`
- Add turn metadata only if current change records cannot identify the producing agent turn.
- Avoid shell-based git restore. Use the existing history snapshot/uncommitted-change rejection path.

### Where It Goes

Primary targets:

- `src/types/uncommitted.ts`
- `src/utils/uncommittedChangesState.ts`
- `src/utils/uncommittedChangeNotifications.ts`
- `src/utils/editorReviewTransitions.ts`
- `src/components/EditorPanel.tsx`
- `src/components/ChatPanel.tsx`
- `src/chat/rendering/ChatViewport.tsx` or the current V3 chat container
- `src/components/editor/GlobalChangeActions.tsx`
- `src-tauri/src/uncommitted_changes.rs`
- `src-tauri/src/commands/uncommitted.rs`
- `src-tauri/src/ai_workflow.rs`
- `src-tauri/src/tools.rs`

Do not start by replacing the existing editor file-change bar. The modal should complement it.

### Why This Enhances Current Behavior

The current system is already safer than the inspiration repo because we track AI edits and provide typed accept/reject commands. The missing product layer is "this agent turn changed N files; review them as a coherent set."

This enhances trust after broad edits:

- Users can understand the turn-level blast radius.
- They can keep good files and revert bad files one by one.
- They can recover after dismissing the banner.
- Multi-file edits become inspectable instead of scattered across tabs.

### Implementation Steps

1. Audit current `UncommittedChange` fields.
2. If needed, extend backend `UncommittedChange` with optional metadata:
   - `conversation_id`
   - `message_id`
   - `turn_id`
   - `created_at_ms`
   - `updated_at_ms`
   - `source` such as `ai_tool`, `semantic_patch`, `manual_review`.
3. Update serialization and project state save/load for added optional fields.
4. Add tracking at edit creation sites:
   - `src-tauri/src/ai_workflow.rs`
   - `src-tauri/src/tools.rs`
   - Any semantic patch path that writes files through AI tools.
5. Build `agentTurnReviewState.ts`:
   - Group uncommitted changes by turn metadata.
   - Fall back to "latest AI changes" when metadata is absent.
   - Mark a group stale if current file modified time no longer matches.
   - Derive counts and status.
6. Add unit tests for grouping:
   - Multiple files same turn.
   - Single file ignored by banner unless configured.
   - Mixed accepted/rejected/pending states.
   - Stale file detection.
7. Add `AgentTurnReviewBanner`:
   - Appears after an agent turn completes and the latest change set has at least two pending files.
   - Shows file count and total additions/deletions.
   - Actions: Review, Keep all, Revert all, Dismiss.
8. Add `AgentTurnReviewModal`:
   - Left file list with pending/kept/reverted status.
   - Main diff area using existing unified diff rendering where possible.
   - Actions: Keep file, Revert file, Open file, Previous, Next, Revert all, Keep all.
9. Wire actions to typed uncommitted commands:
   - Keep file -> `accept_file_changes` or `accept_change`, depending current granularity.
   - Revert file -> `reject_file_changes` or `reject_change`.
   - Keep all -> accept all changes in the group, not necessarily every project change unless the backend supports group ids.
   - Revert all -> reject all changes in the group.
10. If backend cannot accept/reject by group yet, add commands:
   - `accept_changes_by_ids(ids: Vec<String>)`
   - `reject_changes_by_ids(ids: Vec<String>)`
11. After revert, trigger the existing editor review transition:
   - Ensure open files reload authoritatively.
   - Preserve user edits that are not part of the rejected AI change.
12. Add a "last reviewed turn" or "review dismissed" frontend state so the banner does not reopen endlessly.
13. Expose a way to reopen the latest review from the chat turn or change indicator.

### Acceptance Criteria

- After an agent turn modifies two or more files, a review banner appears.
- The modal lists every changed file from that turn with additions/deletions.
- The user can keep one file and revert another without affecting unrelated user changes.
- Reverting a file updates open editor buffers through existing review transitions.
- Revert all applies only to the displayed change set.
- Dismissing the banner does not accept or reject changes.
- Review can be reopened while changes remain pending.
- Tests cover grouping and state transitions.
- No shell command is used to revert files.

### Risks And Mitigations

- Risk: current uncommitted changes may not have turn metadata.
  - Mitigation: ship fallback grouping first, then add metadata for better future behavior.
- Risk: "accept all" accidentally accepts unrelated changes.
  - Mitigation: prefer id-based batch commands over broad `accept_all_changes`.
- Risk: rejecting a file overwrites user edits made after the AI change.
  - Mitigation: use existing stale detection and require confirmation or manual diff when modified time/content hash changed.
- Risk: modal duplicates existing file change bar.
  - Mitigation: keep file-level bar for current file; use modal for turn-level review.

## 4. Fast-Context Project Map Plus Query Spans

### What Goes In

Add two Kortex-inspired context products, but build them from our current index:

1. A compact project symbol map.
2. Query-relevant exact source spans.

Do not port:

- `.aim` storage.
- HRR/vector/convolution code.
- Proxy injection.
- "zero-grep" instructions.
- "trust this map" claims.

New backend structures should be typed and serializable through existing context pack protocol.

Proposed Rust structs:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextProjectMap {
    pub generated_at_ms: i64,
    pub workspace_root: String,
    pub index_status: String,
    pub index_fingerprint: String,
    pub file_count_indexed: usize,
    pub symbol_count_indexed: usize,
    pub cap: ContextProjectMapCap,
    pub files: Vec<ContextProjectMapFile>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextProjectMapCap {
    pub max_files: usize,
    pub max_symbols_per_file: usize,
    pub max_chars: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextProjectMapFile {
    pub path: String,
    pub language: Option<String>,
    pub modified_ms: Option<i64>,
    pub indexed_hash: Option<String>,
    pub freshness: String,
    pub role: Option<String>,
    pub symbols: Vec<ContextProjectMapSymbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextProjectMapSymbol {
    pub name: String,
    pub kind: String,
    pub line_start: u32,
    pub line_end: u32,
    pub signature: Option<String>,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextQuerySpan {
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub byte_start: Option<u32>,
    pub byte_end: Option<u32>,
    pub score: f32,
    pub reason: String,
    pub symbol: Option<String>,
    pub content_hash: String,
    pub preview: String,
    pub warnings: Vec<String>,
}
```

Extend the context pack payload with optional fields:

```rust
pub project_map: Option<ContextProjectMap>,
pub query_spans: Vec<ContextQuerySpan>,
```

Extend request options:

```rust
pub include_project_map: Option<bool>,
pub include_query_spans: Option<bool>,
pub max_project_map_files: Option<usize>,
pub max_query_spans: Option<usize>,
```

### Where It Goes

Primary targets:

- `src-tauri/src/blade_protocol.rs`
- `src-tauri/src/context_pack.rs`
- `src-tauri/src/context_assembly/assembler.rs`
- `src-tauri/src/symbol_index/store.rs`
- `src-tauri/src/symbol_index/search.rs`
- `src-tauri/src/tools.rs`
- `src-tauri/src/ai_workflow/tool_defs.rs`
- `src-tauri/src/blade_ws_client.rs`
- Frontend protocol types under `src/types/blade.ts` if the payload is rendered client-side.

Prefer adding helper modules if `context_pack.rs` becomes too large:

- `src-tauri/src/context_project_map.rs`
- `src-tauri/src/context_query_spans.rs`

### Why This Enhances Current Behavior

Kortex's strongest practical idea is not its storage or math claims. It is the product pattern:

- Give the model a compact project map.
- Then provide exact relevant spans for the current query.

Our implementation can be better because:

- It uses SQLite/FTS/relationships instead of JSON slot scans.
- It can expose index freshness and truncation.
- It can rank by active file/open tabs/preferred directories.
- It can include exact line windows with hashes, so downstream edits can verify what was seen.
- It keeps the output typed, observable, and testable.

This enhances current fast context by adding a stable, broad overview plus exact local evidence in the same request.

### Project Map Ranking Rules

The map must be compact and deterministic. Use these ranking inputs:

1. Active file first.
2. Open tabs next.
3. Files matching query terms.
4. Entry points and config files.
5. Files with high symbol counts or exported/public symbols.
6. Recently indexed or recently modified files.
7. Related files from existing context enrichment.

Per-file symbol ordering:

1. Top-level classes/types/modules.
2. Exported/public functions.
3. Constructors and public methods.
4. Constants/config declarations.
5. Nested/private symbols only if cap remains.

Caps for first implementation:

- `max_files`: 80
- `max_symbols_per_file`: 12
- `max_chars`: 24_000
- `max_query_spans`: 12
- `span_context_lines`: 6 before and 8 after match, unless a symbol range is better.

Every cap breach must set `truncated=true` or add a warning. The prompt must never imply full repository understanding when capped.

### Query Span Ranking Rules

Build spans from three evidence sources:

1. Symbol matches:
   - Existing symbol search for names, qualified names, signatures, and docstrings.
   - Exact symbol range when available.
2. Semantic anchors:
   - Existing semantic anchor search for comments, TODOs, route names, config concepts, and docs.
3. Fallback literal scan:
   - Only when indexed results are weak or empty.
   - Bound file count and bytes read.
   - Respect gitignore/project settings.

Scoring should be explainable:

```text
base score
+ exact symbol name match
+ active file boost
+ open tab boost
+ preferred directory boost
+ semantic anchor match
+ relationship/reference match
- stale index penalty
- fallback literal-only penalty
- generated/vendor path penalty
```

Each span should include a `reason` string such as:

- `exact_symbol_name`
- `semantic_anchor`
- `active_file_reference`
- `related_import`
- `literal_fallback`

### Implementation Steps

1. Add protocol structs in `blade_protocol.rs`.
2. Add `SymbolStore` query helpers if missing:
   - List indexed files with symbol counts.
   - Fetch symbols grouped by file with caps.
   - Fetch symbol count and file count cheaply.
3. Implement `build_context_project_map`.
4. Implement `find_context_query_spans`.
5. Add request parsing in `blade_ws_client.rs`:
   - `include.project_map`
   - `include.projectMap`
   - `include.query_spans`
   - `include.querySpans`
6. Extend `fast_context` tool schema in `ai_workflow/tool_defs.rs`.
7. Extend `fast_context_tool` in `tools.rs`.
8. Integrate into `build_context_pack` with default-off flags.
9. Add prompt-facing summary text only where the chat backend needs text. Keep the canonical result typed.
10. Add warnings:
    - Index unavailable.
    - Index stale.
    - Project map truncated.
    - Query spans used fallback scan.
11. Add Rust tests:
    - Project map is deterministic.
    - Project map caps are enforced and warnings appear.
    - Active file ranks before unrelated indexed files.
    - Query spans return exact line windows and content hashes.
    - Stale index lowers confidence or adds warning.
    - Empty index returns a valid fallback payload.
12. Add a smoke test through `fast_context_tool_returns_context_pack_payload` or adjacent tests.

### Acceptance Criteria

- `fast_context` can request `project_map` and `query_spans`.
- Existing fast-context callers keep the same behavior when flags are omitted.
- Project map includes freshness/cap metadata.
- Query spans include exact paths, line windows, hashes, scores, and reasons.
- Large projects remain bounded by explicit caps.
- The model-facing instructions say "prefer this context first; verify exact files before edits when needed", not "trust this map."
- `cargo test -p zblade context_project_map --lib` or equivalent targeted tests pass.
- `cargo test -p zblade query_spans --lib` or equivalent targeted tests pass.

### Risks And Mitigations

- Risk: project map becomes too large and hurts latency.
  - Mitigation: strict caps, deterministic ranking, cached fingerprint for stable overview.
- Risk: stale indexes mislead the model.
  - Mitigation: expose freshness in both typed payload and prompt text.
- Risk: fallback scans duplicate grep.
  - Mitigation: fallback only when indexed evidence is weak and with small caps.
- Risk: context code grows too large.
  - Mitigation: split project map and query spans into dedicated modules once implementation is non-trivial.

## 5. Active-File Context Panel

### What Goes In

Add a compact context panel that updates with the active editor file.

New frontend pieces:

- `src/components/context/ActiveContextPanel.tsx`
- `src/components/context/ActiveContextSymbols.tsx`
- `src/components/context/ActiveContextRelatedFiles.tsx`
- `src/components/context/ActiveContextIndexHealth.tsx`
- `src/hooks/useActiveFileContext.ts`
- `src/utils/activeFileContextState.ts`
- `src/utils/activeFileContextState.test.ts`

Backend can initially reuse existing commands or add a focused command:

```rust
#[tauri::command]
pub async fn get_active_file_context(
    workspace_root: String,
    file_path: String,
    cursor_line: Option<u32>,
    cursor_column: Option<u32>,
) -> Result<ActiveFileContextPayload, String>
```

Proposed payload:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveFileContextPayload {
    pub path: String,
    pub language: Option<String>,
    pub index_health: IndexHealthSnapshot,
    pub freshness: String,
    pub current_symbol: Option<ActiveContextSymbol>,
    pub symbols: Vec<ActiveContextSymbol>,
    pub related_files: Vec<ActiveContextRelatedFile>,
    pub references: Vec<ActiveContextReference>,
    pub memories: Vec<ActiveContextMemory>,
    pub warnings: Vec<String>,
}
```

Initial UI sections:

- File identity and freshness.
- Current symbol at cursor, when known.
- Top symbols in the file.
- Related files/imports/tests/docs.
- Relevant local memories or rules if available.
- Index health warning when stale or partial.

Do not include ReactFlow or animated graph visualization in the first pass.

### Where It Goes

Primary targets:

- `src/components/Layout.tsx`
- `src/contexts/EditorContext.tsx`
- `src/components/context/*`
- `src/hooks/useActiveFileContext.ts`
- `src-tauri/src/context_assembly/assembler.rs`
- `src-tauri/src/context_pack.rs` or a new backend module
- `src-tauri/src/commands/*` if adding a command
- `src-tauri/src/lib.rs` command registration
- `src/types/blade.ts` or a new frontend type file

The panel should be lazy-loaded from `Layout.tsx`.

### Why This Enhances Current Behavior

The backend already knows a lot about the active file, but most of that value is invisible until the user asks the agent. A context panel makes the index tangible:

- Users can see whether the index understands the current file.
- Related files become discoverable.
- The user can spot stale or missing context before asking for edits.
- It turns fast context from a hidden agent tool into an inspectable workbench feature.

This is the right way to adopt the inspiration repo's active insight idea: text-first, backed by our index, with health metadata.

### Implementation Steps

1. Decide placement:
   - Preferred first pass: right sidebar tab or collapsible panel near chat.
   - Avoid adding a permanent wide panel that shrinks editor space by default.
2. Add lazy import in `Layout.tsx`.
3. Create `useActiveFileContext`:
   - Reads active file path from editor state.
   - Reads cursor position if available.
   - Debounces requests by 250-400ms.
   - Cancels stale requests.
   - Caches the latest payload by `path + cursor symbol + index fingerprint`.
   - Does nothing for untitled/no-file states.
4. Backend first pass:
   - Fetch symbols in current file from `SymbolStore`.
   - Use existing context assembly to identify current symbol and nearby symbols when cursor is provided.
   - Use existing relationship/import helpers to find related files.
   - Include index health.
5. Build UI:
   - Header: path basename and relative directory.
   - Health row: fresh/stale/partial/fallback.
   - Symbol list: name, kind, line range.
   - Related files: path, reason, score if available.
   - Warnings row.
6. Add actions:
   - Click symbol -> reveal line in editor.
   - Click related file -> open file.
   - Copy context -> copy compact text summary.
   - Refresh -> refetch context.
7. Add tests for `activeFileContextState.ts`:
   - Empty state.
   - Fresh payload sorted symbols.
   - Stale warning display state.
   - Related file grouping.
8. Add manual smoke script to the PR description when implemented:
   - Open a Rust/TS file.
   - Move cursor across symbols.
   - Confirm panel updates without UI jank.
   - Confirm stale/partial health is shown if index is unavailable.

### Acceptance Criteria

- Panel is lazy-loaded and does not affect first paint when closed.
- Opening the panel for an indexed file shows symbols and index health.
- Cursor movement updates current symbol without flooding backend requests.
- Related files can be opened from the panel.
- No graph library is loaded in the initial implementation.
- The panel remains useful when the index is stale by showing warnings instead of pretending confidence.
- `bun test src/utils/activeFileContextState.test.ts` passes.
- `bun run build` passes.

### Risks And Mitigations

- Risk: panel causes too many backend calls while typing or moving cursor.
  - Mitigation: debounce, cache, and cancel stale requests.
- Risk: panel crowds the editor.
  - Mitigation: default closed or use existing sidebar/tab surface.
- Risk: users confuse index data with guaranteed completeness.
  - Mitigation: always show freshness and warnings.

## 6. Prefix-Cache-Friendly Context Ordering

### What Goes In

Change prompt/context assembly so stable context appears before volatile context.

This is inspired by Kortex's cache-facing pitch, but with realistic semantics:

- Stable sections may improve provider prompt caching.
- They still consume tokens.
- They must be invalidated when project/index state changes.
- Volatile sections still belong near the user request where helpful.

Add a typed internal representation before rendering prompt text:

```rust
pub struct PromptContextSections {
    pub stable_project: PromptContextSection,
    pub stable_rules: PromptContextSection,
    pub stable_memory: PromptContextSection,
    pub volatile_user_task: PromptContextSection,
    pub volatile_active_file: PromptContextSection,
    pub volatile_query_spans: PromptContextSection,
    pub volatile_tool_state: PromptContextSection,
}

pub struct PromptContextSection {
    pub id: String,
    pub title: String,
    pub body: String,
    pub token_estimate: usize,
    pub fingerprint: Option<String>,
    pub cache_class: PromptCacheClass,
}

pub enum PromptCacheClass {
    Stable,
    SessionStable,
    Volatile,
}
```

Initial ordering:

1. System/developer rules that already must be first.
2. Stable project identity and index health summary.
3. Stable compact project map.
4. Stable project memories/rules that have not changed.
5. Volatile current task/user request.
6. Volatile active file/cursor context.
7. Volatile query spans.
8. Volatile recent tool outputs and editor state.

### Where It Goes

Primary targets:

- `src-tauri/src/chat_orchestrator.rs`
- `src-tauri/src/chat_manager.rs`
- `src-tauri/src/context_pack.rs`
- `src-tauri/src/context_assembly/assembler.rs`
- `src-tauri/src/config.rs` if prompt instructions are centralized there
- `src-tauri/src/tools.rs` if tool output formatting includes fast-context prompt text

Do not scatter ad hoc string concatenation across call sites. Add one assembly helper and route new context sections through it.

### Why This Enhances Current Behavior

The current fast context already has richer data than Kortex. Prefix-cache-friendly ordering makes that data cheaper to reuse when provider caching recognizes common prefixes.

The enhancement is not "zero-token." The enhancement is:

- More deterministic context rendering.
- Better cache locality for stable project overview.
- Cleaner debugging of which context section changed.
- Better budget control because stable and volatile sections can be capped separately.

### Implementation Steps

1. Locate the final prompt assembly path for cloud and local chat requests.
2. Add a small `prompt_context_sections` module or helper in the existing orchestrator area.
3. Define section ids and cache classes.
4. Move current context text generation into sections without changing final text yet.
5. Add tests that assert current output parity for existing simple cases.
6. Add stable/volatile ordering:
   - Stable project map before volatile query spans.
   - Active file and cursor detail after the user task.
   - Tool results remain near the request/turn where they are relevant.
7. Add fingerprints:
   - Project map fingerprint from index status, indexed file hashes, symbol count, and cap settings.
   - Memory fingerprint from memory artifact ids/updated time.
   - Rules fingerprint from settings/rules content hash.
8. Add debug metadata:
   - Section id.
   - Cache class.
   - Char count.
   - Estimated token count.
   - Fingerprint prefix.
9. Add budget logic:
   - Stable project overview has a hard cap.
   - Query spans have a separate cap.
   - Tool outputs keep existing truncation limits.
10. Update prompt wording:
   - Say the project map is an indexed overview.
   - Say caps and freshness are shown.
   - Say exact files should be verified before behavior-changing edits when uncertainty remains.
11. Add tests:
   - Stable sections render before volatile sections.
   - Volatile active-file change does not change stable project fingerprint.
   - Project index fingerprint changes when indexed hashes change.
   - Budget enforcement truncates lower-priority stable symbols before dropping warnings/health.

### Acceptance Criteria

- Existing chat behavior is unchanged when prefix ordering flag is off.
- With flag on, stable project overview renders before volatile request-specific spans.
- Debug logs/metadata identify which section changed between turns.
- Prompt text does not claim zero-token or guaranteed completeness.
- Fast-context caps and freshness are visible to the model.
- Rust tests cover deterministic ordering and fingerprint behavior.

### Risks And Mitigations

- Risk: moving context earlier changes model behavior.
  - Mitigation: flag the change and compare sample conversations before default-on.
- Risk: stable project section becomes stale.
  - Mitigation: include fingerprint and freshness, invalidate on index health changes.
- Risk: local models do not benefit from provider prefix caching.
  - Mitigation: deterministic ordering still improves debuggability and prompt quality; do not overstate savings.

## Cross-Cutting Details

### Feature Flags

Use flags to land independently:

```ts
deferredBootstrap: boolean
terminalFind: boolean
terminalSpawnDiagnostics: boolean
agentTurnReview: boolean
activeContextPanel: boolean
```

Backend:

```rust
context_project_map: bool
context_query_spans: bool
prefix_cache_context_ordering: bool
```

Prefer existing project settings or feature flag modules. Avoid a new untracked config file.

### Telemetry And Debugging

No external telemetry is required. Add local debug/perf markers:

- Deferred init task label and duration.
- Terminal spawn wait duration.
- Agent review change-set id and file count.
- Fast context project map char count, file count, symbol count, truncation.
- Query spans count, source mix, fallback use.
- Prompt section fingerprints and char/token estimates.

### Test Commands

Frontend:

```bash
bun test src/**/*.test.ts
bun run build
```

Backend:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml
```

For large backend changes, run targeted tests first, then the full command.

### UX Principles

- Keep new UI compact and operational.
- Use existing tokens, button styles, icons, and panel patterns.
- No inline-style-heavy ports from the inspiration repo.
- No graph visualization in the first implementation of active context.
- Avoid layout shifts when lazy surfaces mount.
- Use typed backend commands instead of shell command strings.

### Security And Safety

- Preserve workspace boundary checks.
- Do not adopt the inspiration repo's path validation patterns.
- Do not revert files through shell commands.
- Do not tell the model to avoid verification when editing behavior.
- Expose index freshness and truncation wherever context is shown.

## Concrete Backlog

### Task A: Deferred Init Utility

Files:

- `src/utils/deferredInit.ts`
- `src/utils/deferredInit.test.ts`
- `src/App.tsx`
- `src/components/Layout.tsx`

Done when:

- Scheduler exists with cancellation and fallback.
- At least two low-priority startup effects use it.
- Tests and build pass.

### Task B: Terminal Find

Files:

- `package.json`
- `src/components/Terminal.tsx`
- `src/components/terminal/TerminalFindWidget.tsx`
- `public/locales/en/translation.json`
- `public/locales/es/translation.json`

Done when:

- Search widget opens with `Ctrl/Cmd+F`.
- Next/previous and options work.
- No terminal resize or focus regressions.

### Task C: Terminal Spawn Diagnostics

Files:

- `src/components/Terminal.tsx`
- Optional `src/utils/terminalSpawnDiagnostics.ts`

Done when:

- Silent terminals show a local status after timeout.
- First output or exit clears pending diagnostics.
- No polling loop exists.

### Task D: Agent Turn Review State

Files:

- `src/types/uncommitted.ts`
- `src/utils/agentTurnReviewState.ts`
- `src/utils/agentTurnReviewState.test.ts`
- `src-tauri/src/uncommitted_changes.rs`
- `src-tauri/src/commands/uncommitted.rs`

Done when:

- Changes can be grouped by agent turn or fallback group.
- Group actions can target specific change ids.
- Tests cover stale and mixed-status cases.

### Task E: Agent Turn Review UI

Files:

- `src/components/agent-review/AgentTurnReviewBanner.tsx`
- `src/components/agent-review/AgentTurnReviewModal.tsx`
- Current chat container and editor integration files.

Done when:

- Banner appears after multi-file agent edits.
- Modal supports keep/revert per file and group.
- Existing editor review transitions handle buffer reloads.

### Task F: Context Project Map Backend

Files:

- `src-tauri/src/blade_protocol.rs`
- `src-tauri/src/context_project_map.rs`
- `src-tauri/src/context_pack.rs`
- `src-tauri/src/symbol_index/store.rs`
- `src-tauri/src/tools.rs`
- `src-tauri/src/ai_workflow/tool_defs.rs`

Done when:

- `fast_context` can include a capped typed project map.
- Map includes freshness, fingerprints, caps, and warnings.
- Rust tests cover deterministic caps and ordering.

### Task G: Query Spans Backend

Files:

- `src-tauri/src/context_query_spans.rs`
- `src-tauri/src/context_pack.rs`
- `src-tauri/src/symbol_index/search.rs`
- `src-tauri/src/tools.rs`

Done when:

- `fast_context` can include exact ranked query spans.
- Spans include line windows, score, reason, hash, and warning metadata.
- Fallback literal scan is bounded and observable.

### Task H: Prefix Context Sections

Files:

- `src-tauri/src/chat_orchestrator.rs`
- `src-tauri/src/chat_manager.rs`
- `src-tauri/src/context_pack.rs`
- Optional `src-tauri/src/prompt_context_sections.rs`

Done when:

- Prompt context can render stable sections before volatile sections.
- Feature flag controls behavior.
- Fingerprints and debug metadata exist.
- Tests cover ordering and invalidation.

### Task I: Active Context Backend

Files:

- `src-tauri/src/context_assembly/assembler.rs`
- `src-tauri/src/commands/context.rs` or equivalent
- `src-tauri/src/lib.rs`
- Frontend type file.

Done when:

- Active file context payload returns symbols, current symbol, related files, and index health.
- It is bounded, typed, and safe for stale indexes.

### Task J: Active Context Panel

Files:

- `src/components/context/ActiveContextPanel.tsx`
- `src/hooks/useActiveFileContext.ts`
- `src/utils/activeFileContextState.ts`
- `src/utils/activeFileContextState.test.ts`
- `src/components/Layout.tsx`

Done when:

- Panel lazy-loads.
- It updates from active file and cursor with debounce.
- It opens related files and symbols.
- Build and tests pass.

## Definition Of Done

The Adopt work is complete when:

- The six adopted items are implemented behind flags or shipped in stable form.
- The original Kortex-inspired context features are backed by our SQLite symbol index, not `.aim`.
- All new context payloads expose freshness, caps, and warnings.
- Multi-file review uses typed accept/reject or snapshot commands, not shell git commands.
- Terminal search works without weakening the existing Blade event path.
- Startup deferral has measurable debug markers and no first-paint regressions.
- Active context is useful as a text-first panel and does not load graph libraries on startup.
- `bun test src/**/*.test.ts`, `bun run build`, `cargo check --manifest-path src-tauri/Cargo.toml`, and relevant `cargo test` targets are green or any pre-existing failures are documented.

## Explicit Non-Goals

- No rewrite to Zustand or the inspiration repo's state shape.
- No Monaco migration.
- No ReactFlow dependency for the initial active context panel.
- No `.aim` storage.
- No vector/HRR/KV-cache implementation from Kortex.
- No proxy that injects large raw project headers into every request.
- No "zero-token" or "zero-grep" claims.
- No shell-based file revert.
- No frontend terminal polling fallback.
